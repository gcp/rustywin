#[macro_use]
extern crate log;
extern crate env_logger;
extern crate itertools;
extern crate nix;
extern crate libc;

mod display;
use display::*;

mod socket;
use socket::*;

mod client;
use client::*;

use std::io::prelude::*;
use std::io::{Error, ErrorKind};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixStream, UnixListener};
use std::env;
use std::process::{Child, Command};
use log::{LogRecord, LogLevelFilter, SetLoggerError};
use env_logger::LogBuilder;
use nix::sys::select::{FdSet, select};
use nix::c_int;
use nix::sys::time::TimeVal;

const BUFFER_SIZE: usize = 4096;

/// Set up `env_logger` to log from Info and up.
fn setup_logging() -> Result<(), SetLoggerError> {
    let format = |record: &LogRecord| format!("{} - {}", record.level(), record.args());
    let mut builder = LogBuilder::new();
    builder.format(format).filter(None, LogLevelFilter::Info);
    if env::var("RUST_LOG").is_ok() {
        builder.parse(&env::var("RUST_LOG").unwrap());
    }
    builder.init()
}

/// Return the name of our executable if possible.
///
/// This will strip any path components, leaving just the bare command.
///
fn get_exe_name() -> Option<String> {
    let my_name = match env::current_exe() {
        Ok(s) => s,
        Err(_) => {
            error!("Couldn't obtain current executable name");
            return None;
        }
    };
    let my_filename_str = match my_name.file_name() {
        Some(os_str) => os_str.to_str(),
        None => return None,
    };
    match my_filename_str {
        Some(s) => Some(String::from(s)),
        None => None,
    }
}

fn make_coffee_grab_bite(client_stream: &UnixStream,
                         server_stream: &UnixStream)
                         -> Result<(), nix::Error> {
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

fn client_message_loop(mut client_stream: UnixStream, mut server_stream: &UnixStream) {
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
        if let Err(e) = make_coffee_grab_bite(&client_stream, server_stream) {
            break;
        }
    }
}

fn run_unix_socket_loop(sockets: &SocketConnection, mut client_handle: Child) {
    let mut server_stream = sockets.send_stream();

    loop {
        // XXX: Do we need to support multiple clients?
        // XXX: yet both for e10s and certainly e10s-multiple
        // XXX: can test with non-e10s for now tho
        let listen_socket = sockets.listen_socket();
        let client_stream = match listen_socket.accept() {
            Ok((stream, _)) => stream,
            Err(e) => {
                error!("Error accepting socket: {}", e);
                panic!("Broken clients are no good anyway");
                // continue;
            }
        };

        client_message_loop(client_stream, server_stream);

        info!("Waiting for client to exit");
        client_handle.wait().expect("Client exited abornomally");
    }
}

fn main() {
    setup_logging().unwrap();

    info!("Rusty Windows - Starting up");

    let my_name = get_exe_name().expect("Couldn't parse current executable name");

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        writeln!(std::io::stderr(),
                 "Usage: {} <target program> <arguments>",
                 my_name)
            .unwrap();
        std::process::exit(1);
    } else {
        assert!(args.len() >= 2);
        info!("Applying rustywin to \"{}\"", &args[1]);
    }

    // Get the X11 display connection
    let key = "DISPLAY";
    let x11_display = match env::var(key) {
        Ok(val) => {
            info!("{}={}", key, val);
            val
        }
        Err(e) => {
            error!("Couldn't interpret {}: {}", key, e);
            std::process::exit(1);
        }
    };

    let connection = parse_x11_display(x11_display.as_str());

    if connection.is_unix_socket() {
        let sockets = connect_unix_socket(&connection);
        let display_for_client = sockets.get_display();
        let client_handle = launch_client(args[1].as_str(), &args[2..], display_for_client);
        run_unix_socket_loop(&sockets, client_handle);
    }
}
