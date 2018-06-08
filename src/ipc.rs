use std::os::unix::io::RawFd;

use nix;
use nix::sys::socket::{recv, send, MsgFlags};

pub fn send_display(fd: RawFd, display: &str) {
    let mut flags = MsgFlags::empty();
    flags.insert(MsgFlags::MSG_DONTWAIT);

    match send(fd, display.as_bytes(), flags) {
        Err(e) => {
            error!("Couldn't send display string: {}", e);
        }
        Ok(_) => {}
    }
}
