use std::io::prelude::*;
use std::io::Result;
use std::process::{Child, Command};

pub fn launch_client(client_exe: &str, args: &[String], display: &str) -> Child {
    info!("Launching client process \"{}\" args {:?} with DISPLAY=\"{}\"",
          client_exe,
          args,
          display);

    Command::new(client_exe)
        .args(args)
        .env("DISPLAY", display)
        .spawn()
        .expect("Failed to spawn subprocess.")
}