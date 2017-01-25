enum X11ConnectionType {
    Local,
    TCP,
    DECnet,
}

pub struct X11ConnectionDescriptor {
    connection_type: X11ConnectionType,
    host_name: Option<String>,
    server_num: i32,
    screen_num: i32,
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
        Some(idx) => server_screen[idx - 1..].parse::<i32>(),
        None => server_screen.parse::<i32>(),
    };
    // This one is optional so we may take default.
    let screen = match screen_idx {
        Some(idx) => server_screen[idx + 1..].parse::<i32>(),
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