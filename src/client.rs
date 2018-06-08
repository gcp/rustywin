use std::process::Stdio;
use std::process::{Child, Command};

pub fn launch_client(
    client_exe: &str,
    args: &Option<Vec<String>>,
    display: &str,
) -> Child {
    info!(
        "Launching client process \"{}\" args {:?} with DISPLAY=\"{}\"",
        client_exe, args, display
    );

    let args_v = if args.is_some() {
        args.clone().unwrap()
    } else {
        Vec::new()
    };

    Command::new(client_exe)
        .args(args_v)
        .env("DISPLAY", display)
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn subprocess.")
}
