use uuid::Uuid;
use anyhow::{anyhow, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read};
use libflate::zlib;
use crate::server::ClientStatus;
use crate::server::messages::SendMessage;
use std::io;

#[derive(Debug)]
pub struct IdentClientParams {
    game_version: Uuid,
    language: String,
}

#[derive(Debug)]
pub struct LoginClientParams {
    username: String,
    password: String,
}

#[derive(Debug)]
pub enum LoginClientMessage {
    Ident(IdentClientParams),
    Login(LoginClientParams),
}

#[derive(Debug)]
pub struct IdentServerParams {}

#[derive(Debug)]
pub struct WelcomeServerParams {
    pub server_ident: String,
    pub welcome_message: String,
    pub players_total: u32,
    pub players_online: u32,
    pub channels_total: u32,
    pub games_total: u32,
    pub games_running: u32,
    pub games_available: u32,
    pub game_versions: Vec<String>,
    pub initial_channel: String,
}

#[derive(Debug)]
pub struct RejectServerParams {
    pub reason: String,
}

#[derive(Debug)]
pub enum LoginServerMessage {
    Ident(IdentServerParams),
    Welcome(WelcomeServerParams),
    Reject(RejectServerParams),
}

fn decompress_bytes(compressed_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zlib::Decoder::new(compressed_bytes)?;
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

fn compress_bytes(uncompressed_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = zlib::Encoder::new(Vec::new())?;
    io::copy(&mut &uncompressed_bytes[..], &mut encoder)?;
    let mut compressed = encoder.finish().into_result()?;
    let mut final_bytes = Vec::new();
    final_bytes.write_u32::<LittleEndian>(compressed.len() as u32 + 4)?;
    final_bytes.append(&mut compressed);
    Ok(final_bytes)
}

fn try_parse_string(reader: &mut impl ReadBytesExt) -> Result<String> {
    let str_len = reader.read_u32::<LittleEndian>()?;
    let mut str_data = vec![0u8; str_len as usize];
    reader.read_exact(&mut str_data)?;
    Ok(String::from_utf8(str_data)?)
}

fn write_slice(data: &mut Vec<u8>, slice: &[u8]) -> Result<()> {
    data.write_u32::<LittleEndian>(slice.len() as u32)?;
    data.extend_from_slice(slice);
    Ok(())
}

/// Earth uses Windows-style GUID binary representation for its game versions.
/// So we need to carefully parse them to match our UUID format
fn try_parse_guid(reader: &mut impl ReadBytesExt) -> Result<Uuid> {
    let d1 = reader.read_u32::<LittleEndian>()?;
    let d2 = reader.read_u16::<LittleEndian>()?;
    let d3 = reader.read_u16::<LittleEndian>()?;
    let mut d4 = [0; 8];
    reader.read_exact(&mut d4)?;
    let uuid = Uuid::from_fields(d1, d2, d3, &d4)?;
    Ok(uuid)
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

        let decompressed = decompress_bytes(&data[4..compressed_block_end as usize])?;
        drop(data.drain(..compressed_block_end as usize));
        let message = match client_status {
            ClientStatus::Connected => Self::Ident(Self::try_parse_ident(decompressed)?),
            ClientStatus::Greeted => Self::Login(Self::try_parse_login(decompressed)?),
            _ => return Err(anyhow!("Invalid client status")),
        };
        Ok(Some(message))
    }

    fn try_parse_ident(data: Vec<u8>) -> Result<IdentClientParams> {
        let mut cursor = Cursor::new(data);
        let game_version = try_parse_guid(&mut cursor)?;
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

impl SendMessage for LoginServerMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        match self {
            Self::Ident(params) => params.prepare_message(),
            Self::Welcome(params) => params.prepare_message(),
            Self::Reject(params) => params.prepare_message(),
        }
    }
}

impl SendMessage for IdentServerParams {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let mut message = Vec::new();
        // message OK status
        message.write_u32::<LittleEndian>(0)?;
        // TODO: figure out what we should actually send here
        message.write_u32::<LittleEndian>(16u32)?;
        message.write_u32::<LittleEndian>(0x1aff3b3cu32)?;
        message.write_u32::<LittleEndian>(0x1aff3b3cu32)?;
        message.write_u32::<LittleEndian>(0x1aff3b3cu32)?;
        message.write_u32::<LittleEndian>(0x1aff3b3cu32)?;

        Ok(compress_bytes(&message)?)
    }
}

impl SendMessage for WelcomeServerParams {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let mut content = Vec::new();
        write_slice(&mut content, &self.server_ident.as_bytes())?;
        write_slice(&mut content, &self.welcome_message.as_bytes())?;
        // some of these numbers are currently unknown
        content.write_u64::<LittleEndian>(25)?;
        content.write_u32::<LittleEndian>(24)?;
        content.write_u32::<LittleEndian>(self.players_total)?;
        content.write_u32::<LittleEndian>(self.players_online)?;
        content.write_u32::<LittleEndian>(self.channels_total)?;
        // total number of games part a
        content.write_u32::<LittleEndian>(self.games_total)?;
        // total number of games part b (added to a, why?)
        content.write_u32::<LittleEndian>(0)?;
        content.write_u32::<LittleEndian>(18)?;
        // number of games available
        content.write_u32::<LittleEndian>(self.games_available)?;
        content.write_u32::<LittleEndian>(16)?;

        // list of game versions
        for (idx, version) in self.game_versions.iter().enumerate() {
            content.write_u8(idx as u8)?;
            write_slice(&mut content, version.as_bytes())?;
        }
        content.write_u8(0xff)?;  // end of list marker

        // unknown list
        for (idx, version) in self.game_versions.iter().enumerate() {
            content.write_u8(idx as u8)?;
            write_slice(&mut content, version.as_bytes())?;
        }
        content.write_u8(0xff)?;

        // unknown list
        for (idx, version) in self.game_versions.iter().enumerate() {
            content.write_u8(idx as u8)?;
            write_slice(&mut content, version.as_bytes())?;
        }
        content.write_u8(0xff)?;

        // unknown byte
        content.write_u8(0)?;

        // starting channel for the player
        write_slice(&mut content, self.initial_channel.as_bytes())?;

        // unknown u32
        content.write_u32::<LittleEndian>(0)?;
        // unknown bytes, only if prev number is 0? otherwise string-like?
        content.extend_from_slice(&[0u8; 16]);
        // unknown u32
        content.write_u32::<LittleEndian>(0)?;
        // unknown bytes, only if prev number is 0? otherwise string-like?
        content.extend_from_slice(&[0u8; 16]);

        let mut message = Vec::new();
        // message OK status
        message.write_u32::<LittleEndian>(0)?;
        write_slice(&mut message, &content)?;

        Ok(compress_bytes(&message)?)
    }
}

impl SendMessage for RejectServerParams {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let mut content = Vec::new();
        // reject code
        content.write_u32::<LittleEndian>(2)?;
        write_slice(&mut content, self.reason.as_bytes())?;

        Ok(compress_bytes(&content)?)
    }
}
