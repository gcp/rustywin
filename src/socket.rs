use std;
use std::path::Path;

use display::*;
use std::os::unix::net::{UnixStream, UnixListener};
use std::io::prelude::*;
use std::io::{BufReader, BufWriter, Seek, SeekFrom, ErrorKind};
use std::fs::{remove_file, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::env;
use nix::fcntl::{flock, FlockArg};
use nix::unistd::getpid;
use itertools::Itertools;
use libc;

pub struct SocketConnection {
    client_display_name: String,
    client_socket_name: String,
    server_socket_name: String,
}

impl SocketConnection {
    pub fn listen_socket(&self) -> Option<UnixListener> {
        match UnixListener::bind(&self.client_socket_name) {
            Ok(socket) => Some(socket),
            Err(e) => {
                error!("Couldn't bind to listener Unix socket: {}", e);
                None
            }
        }
    }

    pub fn send_stream(&self) -> Option<UnixStream> {
        match UnixStream::connect(&self.server_socket_name) {
            Ok(socket) => Some(socket),
            Err(e) => {
                error!("Couldn't bind to sender Unix socket: {}", e);
                None
            }
        }
    }

    pub fn get_display(&self) -> &str {
        self.client_display_name.as_str()
    }
}

// /tmp/.X11-unix/Xn
// Unix domain socket for display number n
const X11_SOCKET_DIR: &'static str = "/tmp/.X11-unix/";

// Store the list of sockets we create, used for cleanup.
// Format: lines of "pid socket_path"
const X11_SOCKET_LIST: &'static str = ".rustywin_sockets";

pub fn enumerate_unix_x11_sockets() -> Vec<usize> {
    let mut existing_sockets: Vec<usize> = Vec::new();

    let socket_path = Path::new(X11_SOCKET_DIR);
    if !socket_path.is_dir() {
        panic!("No X11 Unix sockets directory ({})", X11_SOCKET_DIR);
    }

    let entries = std::fs::read_dir(socket_path);
    if entries.is_ok() {
        for dir_entry in entries.unwrap() {
            if dir_entry.is_ok() {
                let dir_entry = dir_entry.unwrap();
                let path = dir_entry.path();
                info!("X11 socket found: {}", path.to_string_lossy());
                let file_name = path.file_name().unwrap().to_str().unwrap();
                let x_pos = file_name.rfind('X').unwrap();
                let screen_num = &file_name[x_pos + 1..];
                let screen_num = screen_num.parse::<usize>().unwrap();
                existing_sockets.push(screen_num);
            } else {
                warn!("Can't read directory entry in {:?}", socket_path)
            }
        }
    } else {
        warn!("Can't read X11 socket dir: {}", X11_SOCKET_DIR);
    }

    existing_sockets
}

pub fn cleanup_old_sockets() -> Result<(), std::io::Error> {
    let mut socket_list = env::home_dir().unwrap();
    socket_list.push(X11_SOCKET_LIST);
    let mut file = OpenOptions::new().read(true)
        .write(true)
        .create(true)
        .open(&socket_list)?;
    let fd = file.as_raw_fd();
    if let Err(e) = flock(fd, FlockArg::LockExclusive) {
        warn!("Failed to create file lock on {:?} due to {}",
              socket_list,
              e);
    };
    // Record file contents as we go, we'll drop the lines
    // we're unlinking.
    let mut lines_not_cleaned = Vec::new();

    // "Just a flesh wound"
    // https://github.com/rust-lang/rust/issues/6393#issuecomment-58517921
    {
        let reader = BufReader::new(&mut file);
        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(_) => break,
            };
            let (pid, socket_path) = match line.split_whitespace().next_tuple() {
                Some((x, y)) => (x, y),
                _ => break,
            };
            info!("Old socket for pid {} at {}", pid, socket_path);

            let pid = match pid.parse::<libc::pid_t>() {
                Ok(pid) => pid,
                Err(e) => {
                    warn!("Error parsing pid: {}", e);
                    continue;
                }
            };
            // Check whether the owning process is still alive
            // TOCTTOU is prevented by locking the .rusty_sockets file
            // although we can fail to clean up if a non-rustywin process
            // reuses the pid.
            let res = unsafe { libc::kill(pid, 0) };
            if res != 0 {
                info!("Process {} is dead, cleaning socket {}", pid, socket_path);
                if let Err(e) = remove_file(socket_path) {
                    if e.kind() != ErrorKind::NotFound {
                        warn!("Failed to remove old socket {} due to: {}", socket_path, e);
                        // Process no longer exists but couldn't remove socket
                        lines_not_cleaned.push(line.clone());
                    } else {
                        info!("Socket {} already seems to be deleted.", socket_path);
                    }
                }
            } else {
                // Process still exists
                lines_not_cleaned.push(line.clone());
            }
        }
    }

    // Now rewrite the file, cleaned
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    let mut writer = BufWriter::new(&file);
    for line in &lines_not_cleaned {
        writeln!(writer, "{}", line)?;
    }

    Ok(())
}

fn register_socket_for_cleanup(filename: &str) -> Result<(), std::io::Error> {
    let mut socket_list = env::home_dir().unwrap();
    socket_list.push(X11_SOCKET_LIST);
    let file = OpenOptions::new().append(true)
        .create(true)
        .open(&socket_list)?;
    let fd = file.as_raw_fd();
    if let Err(e) = flock(fd, FlockArg::LockExclusive) {
        warn!("Failed to create file lock on {:?} due to: {}",
              socket_list,
              e);
    };
    let mut writer = BufWriter::new(file);
    writeln!(writer, "{} {}", getpid(), filename)?;
    Ok(())
}

pub fn setup_unix_socket(x11_conn: &X11ConnectionDescriptor) -> SocketConnection {
    let original_server_num = x11_conn.server_num();

    if let Err(e) = cleanup_old_sockets() {
        warn!("Failure cleaning up old sockets: {}", e);
    };

    let existing_servers = enumerate_unix_x11_sockets();
    let free_server_num = match existing_servers.iter().max() {
        // Next available from max
        Some(idx) => idx + 1,
        // Anything is OK, but where's the original connection?
        None => {
            warn!("Was expecting to find an existing screen number.");
            0
        }
    };
    info!("Next available X11 server: #{}", free_server_num);

    let new_unix_socket_name = format!("{}{}{}", X11_SOCKET_DIR, 'X', free_server_num);
    info!("Creating socket at {}", new_unix_socket_name);

    // XXX: We need to recover any old sockets of ours here when we start up,
    // as we can't do that reliably when closing down. Some kind of .rustywin
    // file listing our sockets? With info to determine whether they're in use?
    // libc::atexit is a pain because the path is a global string.
    if let Err(e) = register_socket_for_cleanup(&new_unix_socket_name) {
        warn!("Failure recording sockets in use: {}", e);
    }

    // construct new DISPLAY for client exe
    let client_display_name = format!(":{}", free_server_num);

    // construct path for original X11 socket
    let target_unix_socket_name = format!("{}{}{}", X11_SOCKET_DIR, 'X', original_server_num);

    SocketConnection {
        client_display_name: client_display_name,
        client_socket_name: new_unix_socket_name,
        server_socket_name: target_unix_socket_name,
    }
}
