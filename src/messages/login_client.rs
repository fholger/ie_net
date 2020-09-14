use anyhow::{anyhow, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use libflate::zlib;
use std::io::{Cursor, Read};
use uuid::Uuid;

#[derive(Debug)]
pub struct IdentClientMessage {
    pub game_version: Uuid,
    pub language: String,
}

#[derive(Debug)]
pub struct LoginClientMessage {
    pub username: String,
    pub password: String,
}

fn decompress_bytes(compressed_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zlib::Decoder::new(compressed_bytes)?;
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

fn parse_string(reader: &mut impl ReadBytesExt) -> Result<String> {
    let str_len = reader.read_u32::<LittleEndian>()?;
    let mut str_data = vec![0u8; str_len as usize];
    reader.read_exact(&mut str_data)?;
    Ok(String::from_utf8(str_data)?)
}

/// Earth uses Windows-style GUID binary representation for its game versions.
/// So we need to carefully parse them to match our UUID format
fn parse_guid(reader: &mut impl ReadBytesExt) -> Result<Uuid> {
    let d1 = reader.read_u32::<LittleEndian>()?;
    let d2 = reader.read_u16::<LittleEndian>()?;
    let d3 = reader.read_u16::<LittleEndian>()?;
    let mut d4 = [0; 8];
    reader.read_exact(&mut d4)?;
    let uuid = Uuid::from_fields(d1, d2, d3, &d4)?;
    Ok(uuid)
}

fn try_decompress_block(data: &mut Vec<u8>) -> Result<Option<Vec<u8>>> {
    if data.len() < 4 {
        return Ok(None);
    }

    let mut cursor = Cursor::new(&data);
    let compressed_block_end = cursor.read_u32::<LittleEndian>()?;
    if compressed_block_end > 4096 {
        return Err(anyhow!(
            "Suspiciously large block size, message is assumed invalid"
        ));
    }
    if data.len() < compressed_block_end as usize {
        return Ok(None);
    }

    let decompressed = decompress_bytes(&data[4..compressed_block_end as usize])?;
    data.drain(..compressed_block_end as usize);
    Ok(Some(decompressed))
}

impl IdentClientMessage {
    pub fn try_parse(data: &mut Vec<u8>) -> Result<Option<Self>> {
        if let Some(decompressed) = try_decompress_block(data)? {
            let mut cursor = Cursor::new(decompressed);
            let game_version = parse_guid(&mut cursor)?;
            let language = parse_string(&mut cursor)?;
            return Ok(Some(IdentClientMessage {
                game_version,
                language,
            }));
        }
        Ok(None)
    }
}

impl LoginClientMessage {
    pub fn try_parse(data: &mut Vec<u8>) -> Result<Option<Self>> {
        if let Some(decompressed) = try_decompress_block(data)? {
            let mut cursor = Cursor::new(decompressed);
            let username = parse_string(&mut cursor)?;
            let password = parse_string(&mut cursor)?;
            return Ok(Some(LoginClientMessage { username, password }));
        }
        Ok(None)
    }
}
