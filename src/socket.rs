use std;
use std::path::Path;

use display::*;

pub struct Connection;

// /tmp/.X11-unix/Xn
//  Unix domain socket for display number n
const X11_SOCKET_DIR: &'static str = "/tmp/.X11-unix/";

pub fn enumerate_unix_x11_sockets() -> Vec<usize> {
    let mut existing_sockets: Vec<usize> = Vec::new();

    let socket_path = Path::new(X11_SOCKET_DIR);
    if !socket_path.is_dir() {
        panic!("No X11 Unix sockets directory ({})", X11_SOCKET_DIR);
    }

    let entries = std::fs::read_dir(socket_path);
    if entries.is_ok() {
        for dir_entry in entries.unwrap() {
            if dir_entry.is_ok() {
                let dir_entry = dir_entry.unwrap();
                let path = dir_entry.path();
                info!("X11 socket found: {}", path.to_string_lossy());
                let file_name = path.file_name().unwrap().to_str().unwrap();
                let x_pos = file_name.rfind('X').unwrap();
                let screen_num = &file_name[x_pos + 1..];
                let screen_num = screen_num.parse::<usize>().unwrap();
                existing_sockets.push(screen_num);
            } else {
                warn!("Can't read directory entry in {:?}", socket_path)
            }
        }
    } else {
        warn!("Can't read X11 socket dir: {}", X11_SOCKET_DIR);
    }

    existing_sockets
}

pub fn connect_unix_socket(x11_conn: &X11ConnectionDescriptor) -> Connection {
    let existing_screens = enumerate_unix_x11_sockets();
    let free_screen_num = match existing_screens.iter().max() {
        // Next available
        Some(idx) => idx + 1,
        None => 0
    };

    info!("Next available X11 screen: {}", free_screen_num);

    let display_num = x11_conn.screen_num();

    Connection {}
}