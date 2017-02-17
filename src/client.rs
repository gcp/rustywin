use std::process::{Child, Command, Stdio};

pub fn launch_client(client_exe: &str, args: &[String], display: &str) -> Child {
    info!("Launching client process \"{}\" args {:?} with DISPLAY=\"{}\"",
          client_exe,
          args,
          display);

    Command::new(client_exe)
        .args(args)
        .env("DISPLAY", display)
        //.stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn subprocess.")
}