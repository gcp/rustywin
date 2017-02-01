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

use std::io::Write;
use std::env;
use log::{LogRecord, LogLevelFilter, SetLoggerError};
use env_logger::LogBuilder;

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
            error!("Couldn't obtain current execitable name");
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
    setup_logging().unwrap();

    info!("Rusty Windows - Starting up");

    let my_name = get_exe_name().expect("Couldn't parse current executable name");

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        writeln!(std::io::stderr(), "Usage: {} <target program>", my_name).unwrap();
        std::process::exit(1);
    } else {
        assert_eq!(args.len(), 2);
        info!("Applying rustywin to \"{}\"", &args[1]);
    }

    // Get the X11 display connection
    let key = "DISPLAY";
    let x11_display = match env::var(key) {
        Ok(val) => {
            info!("{}={}", key, val);
            val
        },
        Err(e) => {
            error!("Couldn't interpret {}: {}", key, e);
            std::process::exit(1);
        }
    };

    let connection = parse_x11_display(x11_display.as_str());

    if connection.is_unix_socket() {
        let socket = connect_unix_socket(&connection);
    }
}
