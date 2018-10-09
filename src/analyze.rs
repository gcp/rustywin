use std::fmt::Debug;
use std::fs::File;
use std::io::prelude::*;
use std::str::from_utf8;

use nom::{le_f64, le_i16, le_u16, le_u24, le_u32, le_u8, IResult, Needed};

quick_error! {
    #[derive(Debug)]
    pub enum ParseError {
        ParseFail {
            description("Error parsing X message")
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
struct Request {
    opcode: u8,
    length_4b: u16,
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
named!(
    request<&[u8], Request>,
    do_parse!(
        opcode: le_u8
            >> length_4b: le_u16
            >> (Request {
                opcode: opcode,
                length_4b: length_4b,
            })
    )
);

fn analyze_buffer(buffer: &[u8]) -> ParseResult {
    let size = buffer.len();

    let res = request(buffer);
    println!("{:?}", res);

    Ok(Outcome::Allowed)
}
