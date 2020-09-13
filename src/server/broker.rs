use crate::server::messages::login_server::{
    LoginServerMessage, RejectServerParams, WelcomeServerParams,
};
use crate::server::messages::ServerMessage;
use anyhow::Result;
use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use uuid::Uuid;

pub type Sender<T> = mpsc::UnboundedSender<T>;
pub type Receiver<T> = mpsc::UnboundedReceiver<T>;

pub enum Event {
    NewClient {
        uuid: Uuid,
        messages: Sender<ServerMessage>,
    },
}

pub async fn broker_loop(mut events: Receiver<Event>) -> Result<()> {
    let mut clients: HashMap<Uuid, Sender<ServerMessage>> = HashMap::new();
    log::info!("Main server loop starting up");

    while let Some(event) = events.next().await {
        log::info!("New event");
        match event {
            Event::NewClient { uuid, mut messages } => {
                match clients.entry(uuid) {
                    Entry::Occupied(..) => {
                        // FIXME: actually need to check username, not id
                        messages
                            .send(ServerMessage::Login(LoginServerMessage::Reject(
                                RejectServerParams {
                                    reason: "Already logged in".to_string(),
                                },
                            )))
                            .await?;
                        messages.send(ServerMessage::Disconnect).await?;
                    }
                    Entry::Vacant(entry) => {
                        log::info!("Client {} has successfully logged in", uuid);
                        messages
                            .send(ServerMessage::Login(LoginServerMessage::Welcome(
                                WelcomeServerParams {
                                    server_ident: "IE::Net".to_string(),
                                    welcome_message:
                                        "Welcome to IE::Net, a community-operated EarthNet server"
                                            .to_string(),
                                    players_total: 25,
                                    players_online: 12,
                                    channels_total: 1,
                                    games_total: 0,
                                    games_running: 0,
                                    games_available: 0,
                                    game_versions: vec!["tdm2.1".to_string()],
                                    initial_channel: "General".to_string(),
                                },
                            )))
                            .await?;
                        entry.insert(messages);
                    }
                }
            }
        }
    }

    log::info!("Main server loop shutting down");
    Ok(())
}
