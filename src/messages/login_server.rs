use crate::messages::SendMessage;
use anyhow::Result;
use bytes::BufMut;
use libflate::zlib;
use std::io;

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

fn compress_bytes(uncompressed_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = zlib::Encoder::new(Vec::new())?;
    io::copy(&mut &uncompressed_bytes[..], &mut encoder)?;
    let mut compressed = encoder.finish().into_result()?;
    let mut final_bytes = Vec::new();
    final_bytes.put_u32_le(compressed.len() as u32 + 4);
    final_bytes.append(&mut compressed);
    Ok(final_bytes)
}

fn write_slice(data: &mut Vec<u8>, slice: &[u8]) {
    data.put_u32_le(slice.len() as u32);
    data.extend_from_slice(slice);
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
        message.put_u32_le(0);
        // TODO: figure out what we should actually send here
        message.put_u32_le(16u32);
        message.put_u32_le(0x1aff3b3cu32);
        message.put_u32_le(0x1aff3b3cu32);
        message.put_u32_le(0x1aff3b3cu32);
        message.put_u32_le(0x1aff3b3cu32);

        Ok(compress_bytes(&message)?)
    }
}

impl SendMessage for WelcomeServerParams {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let mut content = Vec::new();
        write_slice(&mut content, &self.server_ident.as_bytes());
        write_slice(&mut content, &self.welcome_message.as_bytes());
        // some of these numbers are currently unknown
        content.put_u64_le(25);
        content.put_u32_le(24);
        content.put_u32_le(self.players_total);
        content.put_u32_le(self.players_online);
        content.put_u32_le(self.channels_total);
        // total number of games part a
        content.put_u32_le(self.games_total);
        // total number of games part b (added to a, why?)
        content.put_u32_le(0);
        content.put_u32_le(18);
        // number of games available
        content.put_u32_le(self.games_available);
        content.put_u32_le(16);

        // list of game versions
        for (idx, version) in self.game_versions.iter().enumerate() {
            content.put_u8(idx as u8);
            write_slice(&mut content, version.as_bytes());
        }
        content.put_u8(0xff); // end of list marker

        // unknown list
        for (idx, version) in self.game_versions.iter().enumerate() {
            content.put_u8(idx as u8);
            write_slice(&mut content, version.as_bytes());
        }
        content.put_u8(0xff);

        // unknown list
        for (idx, version) in self.game_versions.iter().enumerate() {
            content.put_u8(idx as u8);
            write_slice(&mut content, version.as_bytes());
        }
        content.put_u8(0xff);

        // unknown byte
        content.put_u8(0);

        // starting channel for the player
        write_slice(&mut content, self.initial_channel.as_bytes());

        // unknown u32
        content.put_u32_le(0);
        // unknown bytes, only if prev number is 0? otherwise string-like?
        content.extend_from_slice(&[0u8; 16]);
        // unknown u32
        content.put_u32_le(0);
        // unknown bytes, only if prev number is 0? otherwise string-like?
        content.extend_from_slice(&[0u8; 16]);

        let mut message = Vec::new();
        // message OK status
        message.put_u32_le(0);
        write_slice(&mut message, &content);

        Ok(compress_bytes(&message)?)
    }
}

impl SendMessage for RejectServerParams {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let mut content = Vec::new();
        // reject code
        content.put_u32_le(2);
        write_slice(&mut content, self.reason.as_bytes());

        Ok(compress_bytes(&content)?)
    }
}
