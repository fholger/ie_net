pub mod client;
pub mod login_client;
pub mod login_server;

use anyhow::Result;
use std::fmt::Debug;
use crate::messages::client::try_parse_command;

pub trait ServerMessage: Debug + Send + Sync {
    fn prepare_message(&self) -> Result<Vec<u8>>;
}

#[derive(PartialEq, Debug, Default)]
pub struct Command {
    pub command: Vec<u8>,
    pub params: Vec<Vec<u8>>,
}

pub enum ClientMessage {
    Send,
}

impl ClientMessage {
    pub fn try_parse(data: &mut Vec<u8>) -> Result<Option<ClientMessage>> {
        if let Some(position) = data.iter().position(|c| *c == 0) {
            let message_bytes = data.drain(..position);
            let command = try_parse_command(message_bytes.as_slice())?;
            log::debug!("Received command: {:?}", command);
            drop(message_bytes);
            data.drain(..1);
        }
        Ok(None)
    }
}
