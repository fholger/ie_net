use crate::messages::ServerMessage;
use anyhow::Result;

#[derive(Debug)]
pub struct SendMessage {
    pub username: String,
    pub message: Vec<u8>,
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
        Ok(prepare_command("/send", &vec![self.username.as_bytes(), &self.message]))
    }
}
