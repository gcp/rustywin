#[macro_use]
extern crate log;
extern crate env_logger;

use std::io::Write;
use std::env;
use log::{LogRecord, LogLevelFilter, SetLoggerError};
use env_logger::LogBuilder;

fn setup_logging() -> Result<(), SetLoggerError> {
    let format = |record: &LogRecord| {
        format!("{} - {}", record.level(), record.args())
    };
    let mut builder = LogBuilder::new();
    builder.format(format).filter(None, LogLevelFilter::Info);
    if env::var("RUST_LOG").is_ok() {
       builder.parse(&env::var("RUST_LOG").unwrap());
    }
    builder.init()
}

fn main() {
    setup_logging().unwrap();

    info!("Rusty Windows - Starting up");

    let my_fullpath = env::current_exe().unwrap();
    let my_filename = my_fullpath.file_name().unwrap();
    let my_name = my_filename.to_str().unwrap();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        writeln!(std::io::stderr(), "Usage: {} <target program>", my_name).unwrap();
        std::process::exit(1);
    } else {
        assert_eq!(args.len(), 2);
        info!("Applying rustywin to \"{}\"", &args[1]);
    }
}
