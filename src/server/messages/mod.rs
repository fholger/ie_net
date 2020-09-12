pub mod login;

use anyhow::Result;

pub trait SendMessage {
    fn prepare_message(&self) -> Result<Vec<u8>>;
}
