use std;
use std::path::Path;

use display::*;
use std::os::unix::net::{UnixStream, UnixListener};
use std::io::prelude::*;
use std::io::{BufReader, BufWriter, Seek, SeekFrom};
use std::fs::{File, remove_file, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::env;
use nix::fcntl::{flock, FlockArg};
use nix::unistd::getpid;
use itertools::Itertools;
use libc;

pub struct Connection;

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
    // Record file contents as we go, well drop the lines
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
                    warn!("Failed to remove old socket {} due to: {}", socket_path, e);
                    // Process no longer exists but couldn't remove socket
                    lines_not_cleaned.push(line.clone());
                }
            } else {
                // Process still exists
                lines_not_cleaned.push(line.clone());
            }
        }
    }

    // Now rewrite the file, cleaned
    file.seek(SeekFrom::Start(0));
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

pub fn connect_unix_socket(x11_conn: &X11ConnectionDescriptor) -> Connection {
    let original_screen_num = x11_conn.screen_num();

    if let Err(e) = cleanup_old_sockets() {
        warn!("Failure cleaning up old sockets: {}", e);
    };

    let existing_screens = enumerate_unix_x11_sockets();
    let free_screen_num = match existing_screens.iter().max() {
        // Next available from max
        Some(idx) => idx + 1,
        // Anything is OK, but where's the original connection?
        None => {
            warn!("Was expecting to find an existing screen number.");
            0
        }
    };
    info!("Next available X11 screen: #{}", free_screen_num);

    let new_unix_socket_name = format!("{}{}{}", X11_SOCKET_DIR, 'X', free_screen_num);
    info!("Creating socket at {}", new_unix_socket_name);

    // XXX: We need to recover any old sockets of ours here when we start up,
    // as we can't do that reliably when closing down. Some kind of .rustywin
    // file listing our sockets? With info to determine whether they're in use?
    // libc::atexit is a pain because the path is a global string.
    if let Err(e) = register_socket_for_cleanup(&new_unix_socket_name) {
        warn!("Failure recording sockets in use: {}", e);
    }

    let listener = match UnixListener::bind(new_unix_socket_name) {
        Ok(socket) => socket,
        Err(e) => {
            panic!("Couldn't bind to Unix socket: {}", e);
        }
    };


    Connection {}
}
