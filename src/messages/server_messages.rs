use crate::messages::ServerMessage;
use anyhow::Result;

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
pub struct NewGameMessage {
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
        Ok(prepare_command("$user", &vec![self.username.as_bytes(), b"0"]))
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

impl ServerMessage for NewGameMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        Ok(prepare_command("/$play", &vec![&b"00000000-0000-0000-0000-000000000000"[..], self.game_name.as_bytes()]))
    }
}

impl ServerMessage for RawMessage {
    fn prepare_message(&self) -> Result<Vec<u8>> {
        let mut msg_bytes = self.message.as_bytes().to_vec();
        msg_bytes.push(0);
        Ok(msg_bytes)
    }
}
