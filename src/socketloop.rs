use socket::*;

use std::io::prelude::*;
use std::io::ErrorKind;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::process::Child;
use std::thread;

use nix;
use nix::sys::select::{FdSet, select};
use nix::c_int;
use nix::sys::time::TimeVal;

const BUFFER_SIZE: usize = 4096;

pub fn run_unix_socket_loop(sockets: &SocketConnection, mut client_handle: Child) {
    let listen_socket = match sockets.listen_socket() {
        Some(socket) => socket,
        None => return,
    };

    for stream in listen_socket.incoming() {
        match stream {
            Ok(stream) => {
                info!("Successfully accepted a client.");
                handle_client(sockets, stream);
            }
            Err(_) => {
                break;
            }
        }
    }

    drop(listen_socket);

    info!("Waiting for client to exit");
    client_handle.wait().expect("Client exited abornomally");
}

fn handle_client(sockets: &SocketConnection, client_stream: UnixStream) {
    // Incoming connection from client, make our outgoing connection
    // to the original socket.
    let server_stream = match sockets.send_stream() {
        Some(stream) => stream,
        None => {
            error!("Failed to connect to original X11 socket");
            return;
        }
    };

    thread::spawn(|| client_message_loop(client_stream, server_stream));
}

fn client_message_loop(mut client_stream: UnixStream, mut server_stream: UnixStream) {
    server_stream.set_nonblocking(true).expect("Couldn't set sockets to nonblocking");
    client_stream.set_nonblocking(true).expect("Couldn't set sockets to nonblocking");

    loop {
        // XXX: Some canonical way to avoid the useless init?
        let mut buffer: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];

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
        if let Err(e) = select_fds(&client_stream, &server_stream) {
            error!("Error on select: {}", e);
            break;
        }
    }

    info!("Leaving client loop in thread.");
}

fn select_fds(client_stream: &UnixStream, server_stream: &UnixStream) -> Result<(), nix::Error> {
    let mut r_fdset = FdSet::new();
    let client_stream_fd = client_stream.as_raw_fd();
    let server_stream_fd = server_stream.as_raw_fd();
    r_fdset.insert(client_stream_fd);
    r_fdset.insert(server_stream_fd);
    let mut w_fdset = r_fdset.clone();
    let mut e_fdset = r_fdset.clone();
    let mut timeval = TimeVal::zero();
    let nfds = [client_stream_fd as c_int, server_stream_fd as c_int]
        .iter()
        .cloned()
        .max()
        .unwrap();
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