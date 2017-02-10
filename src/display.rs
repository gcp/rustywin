#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum X11ConnectionType {
    Local,
    TCP,
    DECnet,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct X11ConnectionDescriptor {
    connection_type: X11ConnectionType,
    host_name: Option<String>,
    server_num: usize,
    screen_num: usize,
}

impl X11ConnectionDescriptor {
    pub fn is_unix_socket(&self) -> bool {
        self.connection_type == X11ConnectionType::Local
    }

    pub fn server_num(&self) -> usize {
        self.server_num
    }

    pub fn screen_num(&self) -> usize {
        self.screen_num
    }
}

pub fn parse_x11_display(display: &str) -> X11ConnectionDescriptor {
    // DISPLAY = [transport/]host:[:]server[.screen]
    // "unix" as host is the same as empty host, and works as an alias
    // for unix/
    // :: after host signifies DECnet

    // See if transport is specified
    let transport_idx = display.find('/');
    let (transport_str, remainder) = match transport_idx {
        Some(idx) => {
            let (tp, rem) = display.split_at(idx);
            // Remainder still has the / tacked on
            (tp, &rem[1..])
        }
        None => ("unix", display),
    };
    info!("Transport: {}", transport_str);

    let host_idx = remainder.find(':');
    let host = match host_idx {
        Some(idx) => &remainder[0..idx],
        None => panic!("Couldn't parse DISPLAY string."),  // We should find at least one :
    };
    info!("Host: {}", host);

    let connection_type = if remainder.contains("::") {
        warn!("How does one even do DECnet?");
        X11ConnectionType::DECnet
    } else if !host.is_empty() || transport_str.to_lowercase().eq("tcp") {
        X11ConnectionType::TCP
    } else {
        X11ConnectionType::Local
    };

    let server_screen_idx = remainder.rfind(':');
    let server_screen = match server_screen_idx {
        Some(idx) => &remainder[idx + 1..],
        None => panic!("This is logically impossible."),
    };

    let screen_idx = server_screen.find('.');
    // This part must always be present.
    let server = match screen_idx {
        Some(idx) => server_screen[..idx].parse::<usize>(),
        None => server_screen.parse::<usize>(),
    };
    // This one is optional so we may take default.
    let screen = match screen_idx {
        Some(idx) => server_screen[idx + 1..].parse::<usize>(),
        None => Ok(0),
    };

    X11ConnectionDescriptor {
        connection_type: connection_type,
        host_name: if host.is_empty() {
            None
        } else {
            Some(String::from(host))
        },
        server_num: server.expect("Could not parse server value in DISPLAY"),
        screen_num: screen.expect("Could not prase screen number in DISPLAY"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_x11_display() {
        let display = ":0";
        let connection = parse_x11_display(&display);
        assert_eq!(connection.connection_type, X11ConnectionType::Local);
        assert_eq!(connection.host_name, None);
        assert_eq!(connection.screen_num, 0);
        assert_eq!(connection.server_num, 0);
        let display = "mozilla.org:1.2";
        let connection = parse_x11_display(&display);
        assert_eq!(connection.connection_type, X11ConnectionType::TCP);
        assert_eq!(connection.host_name.unwrap(), "mozilla.org");
        assert_eq!(connection.server_num, 1);
        assert_eq!(connection.screen_num, 2);
    }

    #[test]
    #[should_panic]
    fn test_parse_x11_display_fail() {
        let display = "udp/x:0.2";
        parse_x11_display(&display);
        let display = "x:x.2";
        parse_x11_display(&display);
    }
}
