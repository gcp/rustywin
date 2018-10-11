use std::fmt::Debug;
use std::fs::File;
use std::io::prelude::*;
use std::str::from_utf8;

use enum_primitive::FromPrimitive;
use nom::{le_f64, le_i16, le_u16, le_u24, le_u32, le_u8, IResult, Needed};

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
    InternAtom = 0x10,
}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InternAtom<'a> {
    only_if_exists: u8,
    name_length: u16,
    name: &'a [u8],
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
            >> request: take!(length_4b * 4)
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
            >> request: take!(length_4b * 4)
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
        >> if_exists: le_u8
        >> _length: le_u16
        >> name_length: le_u16
        >> _pad: le_u16
        >> name: take!(name_length)
        >> ( InternAtom {
            only_if_exists: if_exists,
            name_length: name_length,
            name: name
        })
    )
);

fn analyze_request_opcode(header: Request, data: &[u8]) -> ParseResult {
    let opcode = Opcode::from_u8(header.opcode);
    println!("{:?}", opcode);

    let result = match opcode {
        Some(Opcode::InternAtom) => {
            let intern = intern_atom(data);
            println!("{:?}", intern);
            Ok(Outcome::Allowed)
        }
        None => Ok(Outcome::Allowed),
    };

    result
}

fn analyze_buffer(mut buffer: &[u8]) -> ParseResult {
    let size = buffer.len();

    while buffer.len() > 0 {
        // Parse request headers
        let req = request(buffer);
        println!("{:?}", req);

        if req.is_ok() {
            let (_, req_header) = req.unwrap();

            if (req_header.length as usize) > size {
                warn!(
                    "Packet size ({}) is smaller than header size ({})",
                    size, req_header.length
                );
                return Err(ParseError::InconsistentLength);
            }

            let decision = analyze_request_opcode(req_header, buffer);
            return decision;
            //if decision.is_ok() {
            //    buffer = &buffer[req_header.length as usize..];
            // }
        }
    }

    Ok(Outcome::Allowed)
}
