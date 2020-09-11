use uuid::Uuid;
use anyhow::{anyhow, Result};
use byteorder::{LittleEndian, ReadBytesExt, BigEndian};
use std::io::{Cursor, Read};
use libflate::zlib;
use crate::server::ClientStatus;

pub struct IdentClientParams {
    game_version: Uuid,
    language: String,
}

pub struct LoginClientParams {
    username: String,
    password: String,
}

pub enum LoginClientMessage {
    Ident(IdentClientParams),
    Login(LoginClientParams),
}

struct IdentServerParams {}

struct WelcomeServerParams {
    welcome_message: String,
}

struct RejectServerParams {
    reason: String,
}

enum LoginServerMessage {
    Ident(IdentServerParams),
    Welcome(WelcomeServerParams),
    Reject(RejectServerParams),
    Disconnect,
}

fn decompress_bytes(compressed_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zlib::Decoder::new(compressed_bytes)?;
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

fn try_parse_string(reader: &mut impl ReadBytesExt) -> Result<String> {
    let str_len = reader.read_u32::<LittleEndian>()?;
    let mut str_data = vec![0u8; str_len as usize];
    reader.read_exact(&mut str_data)?;
    Ok(String::from_utf8(str_data)?)
}

impl LoginClientMessage {
    pub fn try_parse(data: &mut Vec<u8>, client_status: ClientStatus) -> Result<Option<Self>> {
        if data.len() < 4 {
            return Ok(None);
        }

        let mut cursor = Cursor::new(&data);
        let compressed_block_end = cursor.read_u32::<LittleEndian>()?;
        if compressed_block_end > 4096 {
            return Err(anyhow!("Suspiciously large block size, dropping client"));
        }
        if data.len() < compressed_block_end as usize {
            return Ok(None);
        }

        let mut decompressed = decompress_bytes(&data[4..compressed_block_end as usize])?;
        let message = match client_status {
            ClientStatus::Connected => Self::Ident(Self::try_parse_ident(decompressed)?),
            ClientStatus::Greeted => Self::Login(Self::try_parse_login(decompressed)?),
            _ => return Err(anyhow!("Invalid client status")),
        };
        Ok(Some(message))
    }

    fn try_parse_ident(data: Vec<u8>) -> Result<IdentClientParams> {
        let mut cursor = Cursor::new(data);
        let game_version = Uuid::from_u128(cursor.read_u128::<BigEndian>()?);
        let language = try_parse_string(&mut cursor)?;
        Ok(IdentClientParams {
            game_version,
            language,
        })
    }

    fn try_parse_login(data: Vec<u8>) -> Result<LoginClientParams> {
        let mut cursor = Cursor::new(data);
        let username = try_parse_string(&mut cursor)?;
        let password = try_parse_string(&mut cursor)?;
        Ok(LoginClientParams {
            username,
            password,
        })
    }
}