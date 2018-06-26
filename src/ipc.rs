use std::os::unix::io::RawFd;

use nix::sys::socket::{recv, send, MsgFlags};

pub fn send_display(fd: RawFd, display: &str) {
    let mut flags = MsgFlags::empty();
    flags.insert(MsgFlags::MSG_DONTWAIT);

    if let Err(e) = send(fd, display.as_bytes(), flags) {
        error!("Couldn't send display string: {}", e);
    }

    info!("Display string sent: {:?}", display);
}
