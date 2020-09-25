use crate::messages::ServerMessage;
use anyhow::Result;
use nom::AsBytes;
use std::net::Ipv4Addr;
use uuid::Uuid;

#[derive(Debug)]
pub struct SendMessage {
    pub username: String,
    pub message: Vec<u8>,
}

#[derive(Debug)]
pub struct ErrorMessage {
    pub error: String,
}

#[derive(Debug)]
pub struct NewChannelMessage {
    pub channel_name: String,
}

#[derive(Debug)]
pub struct DropChannelMessage {
    pub channel_name: String,
}

#[derive(Debug)]
pub struct NewUserMessage {
    pub username: String,
}

#[derive(Debug)]
pub struct UserJoinedMessage {
    pub username: String,
    pub version_idx: u32,
    pub origin: Option<String>,
}

#[derive(Debug)]
pub struct UserLeftMessage {
    pub username: String,
    pub destination: Option<String>,
}

#[derive(Debug)]
pub struct JoinChannelMessage {
    pub channel_name: String,
}

#[derive(Debug)]
pub struct CreateGameMessage {
    pub version: Uuid,
    pub game_name: String,
    pub password: Vec<u8>,
    pub id: Uuid,
}

#[derive(Debug)]
pub struct JoinGameMessage {
    pub version: Uuid,
    pub game_name: String,
    pub password: Vec<u8>,
    pub ip_addr: Ipv4Addr,
    pub id: Uuid,
}

#[derive(Debug)]
pub struct NewGameMessage {
    pub game_name: String,
    pub id: Uuid,
}

#[derive(Debug)]
pub struct DropGameMessage {
    pub game_name: String,
}

#[derive(Debug)]
pub struct RawMessage {
    pub message: String,
}

fn escape_quotes(input: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(input.len() + 8);
    for b in input {
        if *b == '"' as u8 {
            result.extend_from_slice(b"%22");
        } else {
            result.push(*b);
        }
    }
    result
}

fn prepare_command(command: &str, params: &[&[u8]]) -> Vec<u8> {
    let mut result = Vec::new();
    result.extend_from_slice(command.as_ref());
    for param in params {
        result.push(' ' as u8);
        result.push('"' as u8);
        result.append(&mut escape_quotes(param));
        result.push('"' as u8);
    }
    result.push(0);
    result
}

impl ServerMessage for SendMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command(
            "/send",
            &vec![self.username.as_bytes(), &self.message],
        ))
    }
}

impl ServerMessage for ErrorMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command("/error", &vec![self.error.as_bytes()]))
    }
}

impl ServerMessage for NewChannelMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command(
            "/$channel",
            // TODO: what is the second parameter? game/lang version?
            &vec![self.channel_name.as_bytes(), b"0"],
        ))
    }
}

impl ServerMessage for DropChannelMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command(
            "/&channel",
            &vec![self.channel_name.as_bytes()],
        ))
    }
}

impl ServerMessage for NewUserMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command(
            "$user",
            &vec![self.username.as_bytes(), b"0"],
        ))
    }
}

impl ServerMessage for UserJoinedMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let version = format!("{}", self.version_idx);
        let mut params = vec![self.username.as_bytes(), version.as_bytes()];
        if let Some(origin) = self.origin.as_ref() {
            params.push(origin.as_bytes());
        }
        Ok(prepare_command("/$user", &params))
    }
}

impl ServerMessage for UserLeftMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let mut params = vec![self.username.as_bytes()];
        if let Some(destination) = self.destination.as_ref() {
            params.push(destination.as_bytes());
        }
        Ok(prepare_command("/&user", &params))
    }
}

impl ServerMessage for JoinChannelMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command(
            "/join",
            &vec![self.channel_name.as_bytes()],
        ))
    }
}

impl ServerMessage for CreateGameMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command(
            "/plays",
            &vec![
                self.version.to_hyphenated().to_string().as_bytes(),
                self.game_name.as_bytes(),
                self.password.as_bytes(),
                b"0xcb", // TODO: what does this even mean?
                self.id.to_hyphenated().to_string().as_bytes(),
            ],
        ))
    }
}

impl ServerMessage for JoinGameMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let ip_as_u32 = self
            .ip_addr
            .octets()
            .iter()
            .rev()
            .fold(0u32, |x, y| (x << 8) + (*y as u32));
        Ok(prepare_command(
            "/playc",
            &vec![
                self.version.to_hyphenated().to_string().as_bytes(),
                self.game_name.as_bytes(),
                self.password.as_bytes(),
                format!("0x{:08x}", ip_as_u32).as_bytes(),
                self.id.to_hyphenated().to_string().as_bytes(),
                self.ip_addr.to_string().as_bytes(),
            ],
        ))
    }
}

impl ServerMessage for NewGameMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        // TODO: what do all these extra params actually mean?
        Ok(prepare_command(
            "/$play",
            &vec![
                self.game_name.as_bytes(),
                b"0",
                b"0",
                b"0",
                self.id.to_hyphenated().to_string().as_bytes(),
                b"0",
            ],
        ))
    }
}

impl ServerMessage for DropGameMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command("/&play", &vec![self.game_name.as_bytes()]))
    }
}

impl ServerMessage for RawMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let mut msg_bytes = self.message.as_bytes().to_vec();
        msg_bytes.push(0);
        Ok(msg_bytes)
    }
}
