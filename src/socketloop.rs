use socket::*;

use std::io;
use std::io::prelude::*;
use std::io::ErrorKind;
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Child;
use std::thread;

use std::sync::Arc;
use std::sync::Mutex;

use nix;
use nix::errno::Errno;
use nix::libc::c_int;
use nix::sys::select::{select, FdSet};
use nix::sys::socket::{getsockopt, sockopt};
use nix::Error::Sys;

use analyze;
use ipc;
use DumpFile;

const BUFFER_SIZE: usize = 1 << 16;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum SelectType {
    Readers,
    Writers,
    ReadersAndWriters,
}

pub enum ChildInfo {
    Child(Child),
    RawFd(RawFd),
}

type PidVector = Arc<Mutex<Vec<i32>>>;

trait WriteAllNonBlock {
    /// Similar to write_all, but deals with WouldBlock
    fn write_all_nonblock(
        &mut self,
        write_buff: &[u8],
        child_stderr_fd: &Option<RawFd>,
    ) -> Result<(), io::Error>;
}

impl WriteAllNonBlock for UnixStream {
    fn write_all_nonblock(
        &mut self,
        mut write_buff: &[u8],
        child_stderr_fd: &Option<RawFd>,
    ) -> Result<(), io::Error> {
        loop {
            let written = self.write(&write_buff);
            match written {
                Ok(0) => {
                    return Err(io::Error::new(
                        ErrorKind::WriteZero,
                        "failed to write whole buffer",
                    ))
                }
                Ok(n) => write_buff = &write_buff[n..],
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    // Hold until non-blocking again
                    let mut select_vec = vec![self.as_raw_fd()];
                    if child_stderr_fd.is_some() {
                        let real_child_stderr_fd = child_stderr_fd.unwrap();
                        select_vec.push(real_child_stderr_fd);
                    }
                    if let Err(e) =
                        select_on_vec(&select_vec, SelectType::Writers)
                    {
                        error!("Error during select on write: {}", e);
                        return Err(io::Error::new(
                            ErrorKind::WouldBlock,
                            "Failed to select on handle",
                        ));
                    }
                }
                Err(e) => return Err(e),
            }
            if write_buff.is_empty() {
                break;
            }
        }
        Ok(())
    }
}

pub fn run_unix_socket_loop(
    sockets: SocketConnection,
    listen_socket: UnixListener,
    client_handle: ChildInfo,
    dumpfile: Option<DumpFile>,
) {
    let child_fd = match client_handle {
        ChildInfo::Child(ref child) => {
            // We need the stderr fd number from the child.
            // Given that we wait() on it here and takes a mut ref,
            // we need to extract that fd now.
            // We add this to the select() fdset to ensure all threads get
            // messaged on death.
            // http://stackoverflow.com/a/8976461/909836
            match child.stderr {
                Some(ref stderr) => {
                    info!("Got a handle for stderr, will monitor for exit.");
                    Some(stderr.as_raw_fd())
                }
                None => {
                    info!("Couldn't obtain handle to child's stderr.");
                    None
                }
            }
        }
        ChildInfo::RawFd(rawfd) => Some(rawfd),
    };

    let thread = thread::spawn(move || {
        accept_loop(&sockets, &listen_socket, child_fd, &dumpfile)
    });

    match client_handle {
        ChildInfo::Child(mut child) => {
            info!("Waiting for client to exit");
            child.wait().expect("Client exited abornomally");
        }
        ChildInfo::RawFd(_) => {
            info!("Waiting for thread to exit");
            match thread.join() {
                Ok(_) => {
                    info!("Thread exited normally.");
                }
                Err(e) => {
                    error!("Error joining thread: {:?}", e);
                }
            }
        }
    }
}

pub fn setup_listen_socket(sockets: &SocketConnection) -> Option<UnixListener> {
    match sockets.listen_socket() {
        Some(socket) => Some(socket),
        None => {
            error!("No socket to listen on, nothing to do.");
            None
        }
    }
}

fn accept_loop(
    sockets: &SocketConnection,
    listen_socket: &UnixListener,
    // This is either the stderr fd (for termination)
    // or the socketpair fd (also for comms).
    child_fd: Option<RawFd>,
    dumpfile: &Option<DumpFile>,
) {
    listen_socket
        .set_nonblocking(true)
        .expect("Couldn't set accept loop to nonblocking.");

    let child_pid_vec = PidVector::new(Mutex::new(Vec::new()));

    loop {
        // XXX: This will break if we are working on standalone mode,
        // need to differentiate this use of child_fd.
        // Check whether the master process is sending us
        // some information.
        ipc::try_receive_pids(child_fd, &mut child_pid_vec.lock().unwrap());

        // Check whether a new client is connected
        match listen_socket.accept() {
            Ok((stream, _)) => {
                info!("Successfully accepted a client.");

                handle_client(
                    &sockets,
                    stream,
                    child_fd,
                    child_pid_vec.clone(),
                    dumpfile.clone(),
                );
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => (),
            Err(_) => {
                info!("Error accept()ing on socket.");
                break;
            }
        };

        let mut select_vec = vec![listen_socket.as_raw_fd()];
        if child_fd.is_some() {
            select_vec.push(child_fd.unwrap());
        }
        if let Err(e) =
            select_on_vec(&select_vec, SelectType::ReadersAndWriters)
        {
            error!("Error during select on accept: {}", e);
            return;
        }
    }
}

fn handle_client(
    sockets: &SocketConnection,
    client_stream: UnixStream,
    stderr_fd: Option<RawFd>,
    pid_vector: PidVector,
    dumpfile: Option<DumpFile>,
) {
    // Incoming connection from client, make our outgoing connection
    // to the original socket.
    let server_stream = match sockets.send_stream() {
        Some(stream) => stream,
        None => {
            error!("Failed to connect to original X11 socket");
            return;
        }
    };

    thread::spawn(move || {
        client_message_loop(
            client_stream,
            server_stream,
            stderr_fd,
            pid_vector,
            dumpfile,
        )
    });
}

fn client_message_loop(
    mut client_stream: UnixStream,
    mut server_stream: UnixStream,
    child_stderr_fd: Option<RawFd>,
    pid_vector: PidVector,
    dumpfile: Option<DumpFile>,
) {
    server_stream
        .set_nonblocking(true)
        .expect("Couldn't set sockets to nonblocking");
    client_stream
        .set_nonblocking(true)
        .expect("Couldn't set sockets to nonblocking");

    // Find the PID of our peer
    let client_fd = client_stream.as_raw_fd();

    // This is only supported on non-ARM Linux in nix
    let client_pid;
    if cfg!(all(target_os = "linux", not(target_arch = "arm"))) {
        let creds = sockopt::PeerCredentials;
        let creds_result = getsockopt(client_fd, creds);
        client_pid = creds_result.unwrap().pid();
        info!("Client PID is detected as: {}", client_pid);
    } else {
        client_pid = 0;
    }

    // XXX: Some canonical way to avoid the useless init?
    let mut buffer: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];

    loop {
        let read = match client_stream.read(&mut buffer) {
            Ok(size) => size,
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => 0,
            Err(e) => {
                error!("Read error from socket: {}", e);
                break;
            }
        };

        if read > 0 {
            info!("C->S {} bytes", read);

            let filtered_buffer_pair: (Vec<u8>, Vec<u8>);
            let mut write_buff: &[u8] = &buffer[0..read];

            if !pid_vector.lock().unwrap().contains(&client_pid) {
                filtered_buffer_pair = analyze::filter_buffer(write_buff);
                write_buff = &filtered_buffer_pair.0;
                let reject_buff = filtered_buffer_pair.1;

                info!("Filtering client-server write after harden.");
                // Log traffic that we filter into the dumpfile
                if let Some(ref dump) = dumpfile {
                    match dump.lock().unwrap().write(&buffer) {
                        Ok(_) => (),
                        Err(e) => {
                            error!("Could not write dumpfile: {}", e);
                        }
                    }
                }
            }

            match server_stream
                .write_all_nonblock(&write_buff, &child_stderr_fd)
            {
                Ok(_) => (),
                Err(e) => {
                    info!("Write error on socket: {}", e);
                }
            }
        }

        let read = match server_stream.read(&mut buffer) {
            Ok(size) => size,
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => 0,
            Err(e) => {
                error!("Read error from socket: {}", e);
                break;
            }
        };

        if read > 0 {
            info!("S->C {} bytes", read);
            let write_buff = &buffer[0..read];
            match client_stream
                .write_all_nonblock(&write_buff, &child_stderr_fd)
            {
                Ok(_) => (),
                Err(e) => {
                    info!("Write error on socket: {}", e);
                }
            }
        }

        // Now just block here until anything shows up.
        if let Err(e) = select_streams(
            &client_stream,
            &server_stream,
            child_stderr_fd,
            SelectType::Readers,
        ) {
            error!("Error on select: {}", e);
            break;
        }
    }

    info!("Leaving client loop in thread.");
}

fn select_streams(
    client_stream: &UnixStream,
    server_stream: &UnixStream,
    child_stderr_fd: Option<RawFd>,
    socktype: SelectType,
) -> Result<(), nix::Error> {
    let client_stream_fd = client_stream.as_raw_fd();
    let server_stream_fd = server_stream.as_raw_fd();
    let mut fd_vec = vec![client_stream_fd as c_int, server_stream_fd as c_int];
    if child_stderr_fd.is_some() {
        let real_child_stderr_fd = child_stderr_fd.unwrap();
        fd_vec.push(real_child_stderr_fd);
    }
    select_on_vec(&fd_vec, socktype)
}

fn select_on_vec(
    fdset_vec: &[c_int],
    socktype: SelectType,
) -> Result<(), nix::Error> {
    let mut r_fdset = FdSet::new();
    let mut w_fdset = FdSet::new();
    let mut e_fdset = FdSet::new();
    for fd in fdset_vec {
        match socktype {
            SelectType::Readers => {
                r_fdset.insert(fd.clone());
            }
            SelectType::Writers => {
                w_fdset.insert(fd.clone());
            }
            SelectType::ReadersAndWriters => {
                r_fdset.insert(fd.clone());
                w_fdset.insert(fd.clone());
            }
        };
        e_fdset.insert(fd.clone());
    }
    loop {
        match select(
            None,
            Some(&mut r_fdset),
            Some(&mut w_fdset),
            Some(&mut e_fdset),
            None,
        ) {
            Err(e) => match e {
                Sys(sysno) if sysno == Errno::EINTR => {
                    continue;
                }
                Sys(_) | _ => {
                    error!("Error on select: {}", e);
                    return Err(e);
                }
            },
            Ok(_count) => {
                //info!(
                //    "Found {} awoken fds out of {}.",
                //    count,
                //    3 * fdset_vec.len()
                //);
                return Ok(());
            }
        }
    }
}
