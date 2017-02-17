use socket::*;

use std::io::prelude::*;
use std::io::ErrorKind;
use std::os::unix::io::{RawFd, AsRawFd};
use std::os::unix::net::UnixStream;
use std::process::Child;
use std::thread;

use nix;
use nix::sys::select::{FdSet, select};
use nix::c_int;
use nix::sys::time::TimeVal;

const BUFFER_SIZE: usize = 1 << 16;

pub fn run_unix_socket_loop(sockets: SocketConnection, mut client_handle: Child) {
    // We need the stderr fd number from the child. Given that we wait()
    // on it here and takes a mut ref, we need to extract that fd now.
    // We add this to the select() fdset to ensure all threads get messaged on death.
    // http://stackoverflow.com/a/8976461/909836
    let child_stderr_fd = match client_handle.stderr {
        Some(ref stderr) => {
            info!("Got a handle for stderr, will monitor for exit.");
            Some(stderr.as_raw_fd())
        }
        None => {
            info!("Couldn't obtain handle to child's stderr.");
            None
        }
    };

    thread::spawn(move || accept_loop(sockets, child_stderr_fd));

    info!("Waiting for client to exit");
    client_handle.wait().expect("Client exited abornomally");
}

fn accept_loop(sockets: SocketConnection, stderr_fd: Option<RawFd>) {
    let listen_socket = match sockets.listen_socket() {
        Some(socket) => socket,
        None => {
            error!("No socket to listen on, nothing to do.");
            return;
        }
    };

    listen_socket.set_nonblocking(true).expect("Couldn't set accept loop to nonblocking.");

    loop {
        match listen_socket.accept() {
            Ok((stream, _)) => {
                info!("Successfully accepted a client.");
                handle_client(&sockets, stream, stderr_fd);
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => (),
            Err(_) => {
                info!("Error accept()ing on socket.");
                break;
            }
        };

        let mut select_vec = vec![listen_socket.as_raw_fd()];
        if stderr_fd.is_some() {
            select_vec.push(stderr_fd.unwrap());
        }
        match select_on_vec(select_vec) {
            Err(e) => {
                error!("Error during select on accept: {}", e);
                return;
            }
            Ok(_) => (),
        }
    }
}

fn handle_client(sockets: &SocketConnection, client_stream: UnixStream, stderr_fd: Option<RawFd>) {
    // Incoming connection from client, make our outgoing connection
    // to the original socket.
    let server_stream = match sockets.send_stream() {
        Some(stream) => stream,
        None => {
            error!("Failed to connect to original X11 socket");
            return;
        }
    };

    thread::spawn(move || client_message_loop(client_stream, server_stream, stderr_fd));
}

fn client_message_loop(mut client_stream: UnixStream,
                       mut server_stream: UnixStream,
                       child_stderr_fd: Option<RawFd>) {
    server_stream.set_nonblocking(true).expect("Couldn't set sockets to nonblocking");
    client_stream.set_nonblocking(true).expect("Couldn't set sockets to nonblocking");

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
            if let Err(e) = server_stream.write_all(&buffer[0..read]) {
                if e.kind() != ErrorKind::WouldBlock {
                    error!("Write error on socket: {}", e);
                    break;
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
            if let Err(e) = client_stream.write_all(&buffer[0..read]) {
                if e.kind() != ErrorKind::WouldBlock {
                    error!("Write error on socket: {}", e);
                    break;
                }
            }
        }

        // Now just block here until anything shows up.
        if let Err(e) = select_streams(&client_stream, &server_stream, &child_stderr_fd) {
            error!("Error on select: {}", e);
            break;
        }
    }

    info!("Leaving client loop in thread.");
}

fn select_streams(client_stream: &UnixStream,
              server_stream: &UnixStream,
              child_stderr_fd: &Option<RawFd>)
              -> Result<(), nix::Error> {
    let client_stream_fd = client_stream.as_raw_fd();
    let server_stream_fd = server_stream.as_raw_fd();
    let mut fd_vec = vec![client_stream_fd as c_int, server_stream_fd as c_int];
    if child_stderr_fd.is_some() {
        let real_child_stderr_fd = child_stderr_fd.unwrap();
        fd_vec.push(real_child_stderr_fd);
    }
    select_on_vec(fd_vec)
}

fn select_on_vec(fdset_vec: Vec<c_int>) -> Result<(), nix::Error> {
    let mut r_fdset = FdSet::new();
    for fd in &fdset_vec {
        r_fdset.insert(fd.clone());
    }
    let mut w_fdset = r_fdset.clone();
    let mut e_fdset = r_fdset.clone();
    let mut timeval = TimeVal::zero();
    let nfds = fdset_vec.iter().max().unwrap();
    if let Err(e) = select(nfds + 1,
                           Some(&mut r_fdset),
                           Some(&mut w_fdset),
                           Some(&mut e_fdset),
                           Some(&mut timeval)) {
        info!("Error on select: {}", e);
        return Err(e);
    };
    Ok(())
}