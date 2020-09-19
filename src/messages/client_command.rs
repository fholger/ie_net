use crate::messages::raw_command::{try_parse_raw_command, RawCommand};
use crate::util::bytevec_to_str;
use anyhow::Result;

#[derive(Debug)]
pub enum ClientCommand {
    Send { message: Vec<u8> },
    Join { channel: String },
    HostGame { game_name: String, password: Vec<u8> },
    Unknown { command: String },
    Malformed { reason: String },
}

fn concat_params(params: &[Vec<u8>]) -> Vec<u8> {
    let mut result = Vec::new();
    for (i, param) in params.iter().enumerate() {
        if i != 0 {
            result.push(0x20); // space separator
        }
        result.extend_from_slice(&param);
    }
    result
}

fn send_from_raw(raw: &RawCommand) -> ClientCommand {
    if raw.params.is_empty() {
        return ClientCommand::Malformed {
            reason: "Missing parameters for /send".to_string(),
        };
    }
    ClientCommand::Send {
        message: concat_params(&raw.params[..]),
    }
}

fn join_from_raw(raw: &RawCommand) -> ClientCommand {
    if raw.params.is_empty() {
        return ClientCommand::Malformed {
            reason: "Missing parameters for /join".to_string(),
        };
    }
    ClientCommand::Join {
        channel: String::from_utf8_lossy(&concat_params(&raw.params[..])).to_string(),
    }
}

fn hostgame_from_raw(raw: &RawCommand) -> ClientCommand {
    if raw.params.len() < 3 {
        return ClientCommand::Malformed {
            reason: "Missing parameters for /plays".to_string(),
        };
    }
    ClientCommand::HostGame {
        game_name: String::from_utf8_lossy(&raw.params[1]).to_string(),
        password: raw.params[2].to_vec(),
    }
}

fn match_raw_command(raw: RawCommand) -> ClientCommand {
    match raw.command.as_ref() {
        "send" => send_from_raw(&raw),
        "join" => join_from_raw(&raw),
        "plays" => hostgame_from_raw(&raw),
        _ => ClientCommand::Unknown {
            command: raw.command,
        },
    }
}

impl ClientCommand {
    pub fn try_parse(data: &mut Vec<u8>) -> Result<Option<ClientCommand>> {
        if let Some(position) = data.iter().position(|c| *c == 0) {
            let message_bytes = data.drain(..position + 1);
            log::debug!(
                "Received message: {}",
                bytevec_to_str(message_bytes.as_slice())
            );
            return match try_parse_raw_command(&message_bytes.as_slice()[..position]) {
                Ok(raw) => Ok(Some(match_raw_command(raw))),
                Err(_) => Ok(Some(ClientCommand::Malformed {
                    reason: "Received message is invalid".to_string(),
                })),
            };
        }

        match data.len() {
            n if n > 1024 => Err(anyhow::anyhow!("Message too long")),
            _ => Ok(None),
        }
    }
}
