pub mod raw_command;
pub mod login_client;
pub mod login_server;
pub mod client_command;
pub mod server_messages;

use anyhow::Result;
use std::fmt::Debug;

pub trait ServerMessage: Debug + Send + Sync {
    fn prepare_message(&self) -> Result<Vec<u8>>;
}
