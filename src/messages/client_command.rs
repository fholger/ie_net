use crate::messages::raw_command::{try_parse_raw_command, RawCommand};
use anyhow::Result;
use crate::util::bytevec_to_str;

#[derive(Debug)]
pub enum ClientCommand {
    Send {
        message: Vec<u8>,
    },
    Unknown {
        command: String,
    },
    Malformed {
        reason: String,
    },
}

fn concat_params(params: &[Vec<u8>]) -> Vec<u8> {
    let mut result = Vec::new();
    for param in params {
        result.extend_from_slice(&param);
        result.push(0x20);  // space separator
    }
    result
}

fn send_from_raw(raw: &RawCommand) -> ClientCommand {
    if raw.params.is_empty() {
        return ClientCommand::Malformed {reason: "Missing parameters for /send".to_string()};
    }
    ClientCommand::Send {
        message: concat_params(&raw.params[..]),
    }
}

fn match_raw_command(raw: RawCommand) -> ClientCommand {
    match raw.command.as_ref() {
        "send" => send_from_raw(&raw),
        _ => ClientCommand::Unknown { command: raw.command },
    }
}

impl ClientCommand {
    pub fn try_parse(data: &mut Vec<u8>) -> Result<Option<ClientCommand>> {
        if let Some(position) = data.iter().position(|c| *c == 0) {
            let message_bytes = data.drain(..position + 1);
            log::debug!("Received message: {}", bytevec_to_str(message_bytes.as_slice()));
            return match try_parse_raw_command(message_bytes.as_slice()) {
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
