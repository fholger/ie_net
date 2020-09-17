use anyhow::{anyhow, Result};
use nom::Err::Incomplete;
use nom::IResult;
use nom::Needed::Size;
use uuid::Uuid;

#[derive(Debug)]
pub struct IdentClientMessage {
    pub game_version: Uuid,
    pub language: Vec<u8>,
}

#[derive(Debug)]
pub struct LoginClientMessage {
    pub username: Vec<u8>,
    pub password: Vec<u8>,
}

fn try_parse<T>(data: &mut Vec<u8>, parser: fn(&[u8]) -> IResult<&[u8], T>) -> Result<Option<T>> {
    let (remaining, msg) = match parser(&data) {
        Ok((remaining, ident)) => (remaining.len(), ident),
        Err(Incomplete(Size(n))) if n > 1024 => {
            return Err(anyhow!("Message size {} is too large, assuming error", n))
        }
        Err(Incomplete(_)) => return Ok(None),
        _ => return Err(anyhow!("Error parsing ident message")),
    };
    data.drain(..data.len() - remaining);
    Ok(Some(msg))
}

impl IdentClientMessage {
    pub fn try_parse(data: &mut Vec<u8>) -> Result<Option<Self>> {
        try_parse(data, parsers::compressed_ident_message)
    }
}

impl LoginClientMessage {
    pub fn try_parse(data: &mut Vec<u8>) -> Result<Option<Self>> {
        try_parse(data, parsers::compressed_login_message)
    }
}

mod parsers {
    use crate::messages::login_client::{IdentClientMessage, LoginClientMessage};
    use libflate::zlib;
    use nom::bytes::complete::take;
    use nom::combinator::map_res;
    use nom::multi::count;
    use nom::number::complete::{le_u16, le_u32, le_u8};
    use nom::number::streaming;
    use nom::sequence::tuple;
    use nom::IResult;
    use std::io;
    use std::io::Read;
    use uuid::Uuid;

    /// uses a Windows GUID byte representation, which is a weird mix of byte orderings
    /// we'll read them in groups and feed them to Uuid such that the string representation
    /// matches Earth 2150's original GUID string representation
    fn guid(input: &[u8]) -> IResult<&[u8], Uuid> {
        let parser = tuple((le_u32, le_u16, le_u16, count(le_u8, 8)));
        map_res(parser, |(a, b, c, d)| Uuid::from_fields(a, b, c, &d))(input)
    }

    /// This is a length-delimited block of data where the first 4 bytes
    /// denote the length of the following data block
    /// Expects that all required bytes are present in the input
    fn length_delimited_data(input: &[u8]) -> IResult<&[u8], &[u8]> {
        let (input, length) = le_u32(input)?;
        take(length)(input)
    }

    /// This is a length-delimited block of data where the length includes
    /// the 4 bytes of the length info itself
    /// May return Err::Incomplete
    fn length_delimited_message(input: &[u8]) -> IResult<&[u8], &[u8]> {
        let (input, length) = streaming::le_u32(input)?;
        nom::bytes::streaming::take(length - 4)(input)
    }

    /// Parses a zlib-compressed message and returns the uncompressed data
    pub fn compressed_message(input: &[u8]) -> IResult<&[u8], Vec<u8>> {
        map_res(
            length_delimited_message,
            |compressed| -> io::Result<Vec<u8>> {
                let mut decoder = zlib::Decoder::new(compressed)?;
                let mut decompressed = Vec::new();
                decoder.read_to_end(&mut decompressed)?;
                Ok(decompressed)
            },
        )(input)
    }

    fn ident_message(input: &[u8]) -> IResult<&[u8], IdentClientMessage> {
        let (input, guid) = guid(input)?;
        let (input, lang) = length_delimited_data(input)?;
        Ok((
            input,
            IdentClientMessage {
                game_version: guid,
                language: lang.to_vec(),
            },
        ))
    }

    pub fn compressed_ident_message(input: &[u8]) -> IResult<&[u8], IdentClientMessage> {
        map_res(
            compressed_message,
            |decompressed| -> Result<IdentClientMessage, ()> {
                match ident_message(&decompressed) {
                    Ok((_, ident)) => Ok(ident),
                    _ => Err(()),
                }
            },
        )(input)
    }

    fn login_message(input: &[u8]) -> IResult<&[u8], LoginClientMessage> {
        let (input, username) = length_delimited_data(input)?;
        let (input, password) = length_delimited_data(input)?;
        Ok((
            input,
            LoginClientMessage {
                username: username.to_vec(),
                password: password.to_vec(),
            },
        ))
    }

    pub fn compressed_login_message(input: &[u8]) -> IResult<&[u8], LoginClientMessage> {
        map_res(
            compressed_message,
            |decompressed| -> Result<LoginClientMessage, ()> {
                match login_message(&decompressed) {
                    Ok((_, login)) => Ok(login),
                    _ => Err(()),
                }
            },
        )(input)
    }

    #[cfg(test)]
    mod test {
        use crate::messages::login_client::parsers::guid;
        use std::str::FromStr;
        use uuid::Uuid;

        #[test]
        fn test_guid() {
            let bytes = [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f,
            ];
            assert_eq!(
                guid(&bytes),
                Ok((
                    &b""[..],
                    Uuid::parse_str("03020100-0504-0706-0809-0a0b0c0d0e0f").unwrap()
                ))
            )
        }
    }
}
