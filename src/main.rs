#![cfg_attr(feature = "clippy", feature(plugin))]
#![cfg_attr(feature = "clippy", plugin(clippy))]

#[macro_use]
extern crate log;
#[macro_use]
extern crate clap;
extern crate byteorder;
extern crate dirs;
extern crate env_logger;
extern crate itertools;
extern crate libc;
extern crate nix;

mod client;
mod display;
mod ipc;
mod socket;
mod socketloop;

use clap::{App, AppSettings, Arg};
use env_logger::{Builder, Env};
use socketloop::ChildInfo;
use std::env;

/// Set up `env_logger` to log from Info and up.
fn setup_logging() {
    let env = Env::default().filter_or("RUST_LOG", "info");

    let mut builder = Builder::from_env(env);
    builder.default_format_timestamp(false);
    builder.init();
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

fn main() {
    setup_logging();

    let my_name =
        get_exe_name().expect("Couldn't parse current executable name");

    let matches = App::new("Rusty Windows")
        .version(crate_version!())
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .author(crate_authors!())
        .setting(AppSettings::TrailingVarArg)
        .arg(
            Arg::with_name("fd")
                .short("f")
                .long("fd")
                .help("Starts as server communicating on fd#")
                .takes_value(true)
                .number_of_values(1)
                .required(true)
                .display_order(1)
                .conflicts_with("target"),
        )
        .arg(
            Arg::with_name("target")
                .help("Launches the target program")
                .index(1)
                .required(true)
                .conflicts_with("fd"),
        )
        .arg(
            Arg::with_name("target_args")
                .help("Arguments for the target program")
                .index(2)
                .multiple(true)
                .requires("target"),
        )
        .get_matches();

    info!("Rusty Windows - Starting up");

    if matches.is_present("target") {
        info!(
            "Applying {} to \"{}\"",
            my_name,
            matches.value_of("target").unwrap()
        );
        if matches.is_present("target_args") {
            let arguments = matches.values_of_lossy("target_args").unwrap();
            info!("Args: {:?}", arguments);
        }
    }

    let target = matches.value_of("target");
    let args = matches.values_of_lossy("target_args");
    let fd = match matches.value_of("fd") {
        Some(fd) => fd.parse::<i32>().ok(),
        None => None,
    };

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

    let connection = display::parse_x11_display(x11_display.as_str());

    if connection.is_unix_socket() {
        let sockets = socket::setup_unix_socket(&connection);
        // The listen socket needs to be up before we launch the client.
        let listen_socket = match socketloop::setup_listen_socket(&sockets) {
            Some(socket) => socket,
            None => std::process::exit(1),
        };

        // to_string() is needed here to break the lifetime link between
        // sockets and (eventually) client_handle.
        let display_for_client = sockets.get_display().to_string();

        // Now either get a handle to the child (from which we will extract
        // standards fds) or the fd to listen to.
        let client_handle = if target.is_some() {
            ChildInfo::Child(client::launch_client(
                &target.unwrap().to_string(),
                &args,
                display_for_client.as_str(),
            ))
        } else {
            assert!(fd.is_some());
            info!("Socket FD: {:?}", fd.unwrap());
            ChildInfo::RawFd(fd.unwrap())
        };

        // We've been given an fd corresponding to a socketpair to
        // communicate over. Send our X DISPLAY var.
        if fd.is_some() {
            ipc::send_display(fd.unwrap(), sockets.get_display())
        }

        socketloop::run_unix_socket_loop(sockets, listen_socket, client_handle);
    }
}
