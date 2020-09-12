pub mod login;

use anyhow::Result;
use crate::server::messages::login::LoginServerMessage;

pub trait SendMessage {
    fn prepare_message(&self) -> Result<Vec<u8>>;
}

#[derive(Debug)]
pub enum ServerMessage {
    Login(LoginServerMessage),
    Disconnect,
}

pub enum ClientMessage {
    Send,
}

impl ClientMessage {
    pub fn try_parse(data: &mut Vec<u8>) -> Result<Option<ClientMessage>> {
        if let Some(position) = data.iter().position(|c| { *c == 0 }) {
            let message_bytes = data.drain(..position);
            let message = String::from_utf8_lossy(message_bytes.as_slice());
            log::debug!("Received message: {}", message);
            drop(message_bytes);
            data.drain(..1);
        }
        Ok(None)
    }
}