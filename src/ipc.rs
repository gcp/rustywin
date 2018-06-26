extern crate byteorder;

use std::os::unix::io::RawFd;

use byteorder::{ByteOrder, NativeEndian};
use nix::errno;
use nix::fcntl;
use nix::fcntl::OFlag;
use nix::sys::socket::{recv, send, MsgFlags};
use nix::Error;

pub fn send_display(fd: RawFd, display: &str) {
    let mut flags = MsgFlags::empty();
    flags.insert(MsgFlags::MSG_DONTWAIT);

    if let Err(e) = send(fd, display.as_bytes(), flags) {
        error!("Couldn't send display string: {}", e);
    }

    info!("Display string sent: {:?}", display);
}

fn process_pid_message(cmd: u8, pid: u32, pids: &mut Vec<u32>) {
    if cmd == 0 {
        if !pids.contains(&pid) {
            pids.push(pid);
            info!("Added PID {}, PIDS={:?}", pid, pids);
        } else {
            info!("Already contains PID {}, PIDS={:?}", pid, pids);
        }
    } else if cmd == 1 {
        pids.retain(|&x| x != pid);
        info!("Removed PID {}, PIDS={:?}", pid, pids);
    }
}

pub fn try_receive_pids(fd: Option<RawFd>, pids: &mut Vec<u32>) {
    if fd.is_none() {
        return;
    }
    let fd = fd.unwrap();
    let flags = fcntl::FcntlArg::F_SETFL(OFlag::O_NONBLOCK);

    if let Err(e) = fcntl::fcntl(fd, flags) {
        panic!("Couldn't set comms socket to nonblocking: {}", e);
    }

    const BUFSIZE: usize = 16;
    let mut buffer: [u8; BUFSIZE] = [0; BUFSIZE];

    let flags = MsgFlags::empty();
    match recv(fd, &mut buffer, flags) {
        Ok(n) => {
            info!("Received {} bytes from parent", n);
            if n != 5 {
                panic!("Unexpected message length: {}", n);
            }
            let cmd = buffer[0];
            let pid = NativeEndian::read_u32(&buffer[1..5]);
            process_pid_message(cmd, pid, pids);
        }
        Err(e) => {
            if let Error::Sys(err) = e {
                if err == errno::EWOULDBLOCK {
                    // Not a real failure, just no data.
                    return;
                }
            };
            panic!("Error receiving parent info: {:?}", e);
        }
    }
}
