#[macro_use]
extern crate log;
extern crate env_logger;

use std::io::Write;
use std::env;
use log::{LogRecord, LogLevelFilter, SetLoggerError};
use env_logger::LogBuilder;

fn setup_logging() -> Result<(), SetLoggerError> {
    let format = |record: &LogRecord| format!("{} - {}", record.level(), record.args());
    let mut builder = LogBuilder::new();
    builder.format(format).filter(None, LogLevelFilter::Info);
    if env::var("RUST_LOG").is_ok() {
        builder.parse(&env::var("RUST_LOG").unwrap());
    }
    builder.init()
}

fn get_exe_name() -> Option<String> {
    let my_name = match env::current_exe() {
        Ok(t) => t,
        Err(_) => {
            error!("Couldn't parse exe name");
            return None;
        }
    };
    let my_filename = my_name.file_name();
    match my_filename {
        Some(y) => Some(String::from(y.to_string_lossy())),
        _ => None,
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
}
