use std::borrow::Cow;
use std::fmt::Debug;
use std::fs::File;
use std::io::prelude::*;

use enum_primitive::FromPrimitive;
use nom::{le_i16, le_u16, le_u24, le_u32, le_u8, IResult, Needed};

quick_error! {
    #[derive(Debug)]
    pub enum ParseError {
        ParseFail {
            description("Error parsing X message")
        }
        InconsistentLength {
            description("Message length inconsistent")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Allowed,
    Denied,
}

type ParseResult = Result<Outcome, ParseError>;

pub fn analyze_file(filename: &str) -> ParseResult {
    let mut f = File::open(filename).expect("File not found.");

    let mut buffer = Vec::new();

    // read the whole file
    f.read_to_end(&mut buffer).expect("Error reading dumpfile.");

    analyze_buffer(&buffer)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Request<'a> {
    opcode: u8,
    datab: u8,
    length: u32,
    data: &'a [u8],
}

enum_from_primitive! {
#[derive(Debug, PartialEq)]
// enum with explicit discriminator
enum Opcode {
    ChangeWindowAttributes = 0x2,
    InternAtom = 0x10,
    ChangeProperty = 0x12,
    GetProperty = 0x14,
    GrabButton = 0x1C,
    QueryExtension = 0x62,
}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InternAtom<'a> {
    only_if_exists: bool,
    name_length: u16,
    name: Cow<'a, str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GetProperty {
    delete: bool,
    window: u32,
    property: u32,
    atom_prop_type: u32,
    offset: u32,
    length: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QueryExtension<'a> {
    name_length: u16,
    name: Cow<'a, str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChangeProperty<'a> {
    mode: u8,
    window: u32,
    property: u32,
    prop_type: u32,
    format: u8,
    data_length: u32,
    data: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GrabButton {
    owner_events: u8,
    window: u32,
    // ignore
}

// Every request contains an 8-bit major opcode and a 16-bit length
// field expressed in units of four bytes. Every request consists of
// four bytes of a header (containing the major opcode, the length field,
// and a data byte) followed by zero or more additional bytes of data.
// The length field defines the total length of the request, including
// the header. The length field in a request must equal the minimum length
// required to contain the request. If the specified length is smaller or
// larger than the required length, an error is generated. Unused bytes
// in a request are not required to be zero. Major opcodes 128 through 255
// are reserved for extensions. Extensions are intended to contain multiple
// requests, so extension requests typically have an additional minor
// opcode encoded in the second data byte in the request header.
//
// gcp: It is of course totally obvious from this explanation that the
// data byte sits in between the opcode and request length, right?

// BIG-REQUESTS
// This extension defines a mechanism for extending the length field beyond
// 16 bits. If the normal 16-bit length field of the protocol request is zero,
// then an additional 32-bit field containing the actual length
// (in 4-byte units) is inserted into the request, immediately following
// the 16-bit length field.

// Good references:
// https://github.com/boundary/wireshark/blob/master/epan/dissectors/packet-x11.c
// https://cgit.freedesktop.org/xorg/app/xscope/tree/x11.h#n406

named!(
    request<&[u8], Request>,
    alt!(
        // Using BIGREQUEST -> length == 0
        do_parse!(
            opcode: le_u8
            >> datab: le_u8
            >> _length_zero: tag!(b"\x00\x00")
            >> length_4b: le_u32
            >> request: take!(length_4b * 4 - (4 + 2 + 1 + 1))
            >> (Request {
                    opcode: opcode,
                    datab: datab,
                    length: 4 * length_4b,
                    data: request
                })
        )
        |
        // Normal request
        do_parse!(
            opcode: le_u8
            >> datab: le_u8
            >> length_4b: le_u16
            >> request: take!(length_4b * 4 - (2 + 1 + 1))
            >> (Request {
                    opcode: opcode,
                    datab: datab,
                    length: 4 * length_4b as u32,
                    data: request
                })
        )
    )
);

named!(intern_atom<&[u8], InternAtom>,
    do_parse!(
        _opcode: le_u8
        >> only_if_exists: le_u8
        >> _length: le_u16
        >> name_length: le_u16
        >> _pad: le_u16
        >> name: take!(name_length)
        >> ( InternAtom {
                only_if_exists: only_if_exists == 1,
                name_length: name_length,
                name: String::from_utf8_lossy(name)
        })
    )
);

named!(getproperty<&[u8], GetProperty>,
    do_parse!(
        _opcode: le_u8
        >> delete: le_u8
        >> _length: le_u16
        >> window: le_u32
        >> property: le_u32
        >> atom_prop_type: le_u32
        >> offset: le_u32
        >> length: le_u32
        >> ( GetProperty {
                delete: delete == 1,
                window: window,
                property: property,
                atom_prop_type: atom_prop_type,
                offset: offset,
                length: length,
            })
    )
);

named!(queryextension<&[u8], QueryExtension>,
    do_parse!(
        _opcode: le_u8
        >> _dummy: le_u8
        >> _length: le_u16
        >> name_length: le_u16
        >> _pad: le_u16
        >> name: take!(name_length)
        >> ( QueryExtension {
                name_length: name_length,
                name: String::from_utf8_lossy(name)
            })
    )
);

named!(changeproperty<&[u8], ChangeProperty>,
    do_parse!(
        _opcode: le_u8
        >> mode: le_u8
        >> _length: le_u16
        >> window: le_u32
        >> property: le_u32
        >> prop_type: le_u32
        >> prop_format: le_u8
        >> _pad: le_u24
        >> data_length: le_u32
        >> data: take!(data_length)
        >> (ChangeProperty {
               mode: mode,
               window: window,
               property: property,
               prop_type: prop_type,
               format: prop_format,
               data_length: data_length,
               data: data,
        })
    )
);

named!(grabbutton<&[u8], GrabButton>,
    do_parse!(
        _opcode: le_u8
        >> owner_events: le_u8
        >> length: le_u16
        >> window: le_u32
        >> _data: take!(length - 4 + 2 + 2)
        >> (GrabButton {
               owner_events: owner_events,
               window: window,
        })
    )
);

fn analyze_request_opcode(header: Request, data: &[u8]) -> ParseResult {
    let opcode = Opcode::from_u8(header.opcode);

    let result = match opcode {
        Some(Opcode::InternAtom) => {
            let intern = intern_atom(data);
            if intern.is_ok() {
                println!("{:?}", intern.unwrap().1);
            } else {
                println!("{:?}", intern);
            }
            Ok(Outcome::Allowed)
        }
        Some(Opcode::GetProperty) => {
            let getprop = getproperty(data);
            if getprop.is_ok() {
                println!("{:?}", getprop.unwrap().1);
            } else {
                println!("{:?}", getprop);
            }
            Ok(Outcome::Allowed)
        }
        Some(Opcode::QueryExtension) => {
            let queryext = queryextension(data);
            if queryext.is_ok() {
                println!("{:?}", queryext.unwrap().1);
            } else {
                println!("{:?}", queryext);
            }
            Ok(Outcome::Allowed)
        }
        Some(Opcode::ChangeProperty) => {
            let changeprop = changeproperty(data);
            if changeprop.is_ok() {
                println!("{:?}", changeprop.unwrap().1);
            } else {
                println!("{:?}", changeprop);
            }
            Ok(Outcome::Allowed)
        }
        Some(Opcode::GrabButton) => {
            let grab = grabbutton(data);
            if grab.is_ok() {
                println!("{:?}", grab.unwrap().1);
            } else {
                println!("{:?}", grab);
            }
            Ok(Outcome::Allowed)
        }
        None => Ok(Outcome::Allowed),
        _ => {
            println!("{:?}", opcode);
            Ok(Outcome::Allowed)
        }
    };

    result
}

/// Filters the buffer with X commands. Returns two buffers,
/// one with accepted and one with rejected requests.
pub fn filter_buffer(buffer: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut out_reject_buff = Vec::with_capacity(buffer.len());
    let mut out_accept_buff = Vec::with_capacity(buffer.len());
    let mut work_buffer = &buffer[0..buffer.len()];

    while buffer.len() > 0 {
        let size = work_buffer.len();
        println!("Buffer size={}", size);

        // Parse request headers
        let req = request(work_buffer);
        if req.is_err() {
            out_reject_buff.extend(&work_buffer[0..]);
            break;
        }

        let (_, req_header) = req.unwrap();
        println!("{:?}", req_header);

        if (req_header.length as usize) > size {
            warn!(
                "Packet size ({}) is smaller than header size ({})",
                size, req_header.length
            );
            out_reject_buff.extend(&work_buffer[0..]);
            break;
        }

        let decision = analyze_request_opcode(req_header, work_buffer);
        println!("{:?}", decision);
        match decision {
            Ok(Outcome::Allowed) => {
                out_accept_buff
                    .extend(&work_buffer[0..req_header.length as usize]);
            }
            Ok(Outcome::Denied) => {
                out_reject_buff
                    .extend(&work_buffer[0..req_header.length as usize]);
            }
            Err(_) => {
                out_accept_buff
                    .extend(&work_buffer[0..req_header.length as usize]);
            }
        }
        if decision.is_ok() {
            println!("Skipping {} bytes...", req_header.length);
            work_buffer = &work_buffer[req_header.length as usize..];
        }
    }

    println!(
        "Accepted {} bytes, rejected {} bytes",
        out_accept_buff.len(),
        out_reject_buff.len(),
    );
    (out_accept_buff, out_reject_buff)
}

fn analyze_buffer(mut buffer: &[u8]) -> ParseResult {
    while buffer.len() > 0 {
        let size = buffer.len();
        println!("Buffer size={}", size);

        // Parse request headers
        let req = request(buffer);

        if req.is_ok() {
            let (_, req_header) = req.unwrap();
            println!("{:?}", req_header);

            if (req_header.length as usize) > size {
                warn!(
                    "Packet size ({}) is smaller than header size ({})",
                    size, req_header.length
                );
                return Err(ParseError::InconsistentLength);
            }

            let decision = analyze_request_opcode(req_header, buffer);
            println!("{:?}", decision);
            if decision.is_ok() {
                println!("Skipping {} bytes...", req_header.length);
                buffer = &buffer[req_header.length as usize..];
            }
        } else {
            break;
        }
    }

    Ok(Outcome::Allowed)
}

#[cfg(test)]
mod tests {
    use super::*;
    const D_INTERNATOM: &'static [u8] = include_bytes!("../dumps/blocked.dmp");

    #[test]
    fn test_request() {
        let req = request(D_INTERNATOM);
        let req_header = req.unwrap().1;
        assert_eq!(
            req_header,
            Request {
                opcode: 16,
                datab: 0,
                length: 32,
                data: &[
                    21, 0, 64, 7, 95, 71, 84, 75, 95, 69, 68, 71, 69, 95, 67,
                    79, 78, 83, 84, 82, 65, 73, 78, 84, 83, 47, 109, 111
                ]
            }
        );
        let ia = intern_atom(D_INTERNATOM);
        let ia = ia.unwrap().1;
        assert_eq!(
            ia,
            InternAtom {
                only_if_exists: false,
                name_length: 21,
                name: std::borrow::Cow::from("_GTK_EDGE_CONSTRAINTS"),
            },
        );
    }
}
