use crate::messages::login_server::{RejectServerMessage, WelcomeServerMessage};
use crate::messages::ServerMessage;
use anyhow::Result;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

pub type Sender<T> = mpsc::Sender<T>;
pub type Receiver<T> = mpsc::Receiver<T>;

struct Broker {
    clients: HashMap<Uuid, Client>,
}

#[derive(Debug)]
pub enum Event {
    NewClient {
        id: Uuid,
        username: String,
        send: Sender<Box<dyn ServerMessage>>,
    },
    DropClient {
        id: Uuid,
    },
}

struct Client {
    username: String,
    send: Sender<Box<dyn ServerMessage>>,
}

impl Broker {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }
    async fn handle_event(&mut self, event: Event) -> Result<()> {
        log::info!("New event");
        match event {
            Event::NewClient {
                id,
                username,
                mut send,
            } => {
                match self.clients.entry(id) {
                    Entry::Occupied(..) => {
                        // FIXME: actually need to check username, not id
                        send.send(Box::new(RejectServerMessage {
                            reason: "Already logged in".to_string(),
                        }))
                        .await?;
                    }
                    Entry::Vacant(entry) => {
                        log::info!("Client {} has successfully logged in", id);
                        send.send(Box::new(WelcomeServerMessage {
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
                        }))
                        .await?;
                        entry.insert(Client { username, send });
                    }
                }
            }
            Event::DropClient { id } => {
                log::info!("Client {} disconnected, dropping", id);
                self.clients.remove(&id);
            }
        }
        Ok(())
    }
}

pub async fn broker_loop(
    mut events: Receiver<Event>,
    mut shutdown_recv: watch::Receiver<bool>,
) -> Result<()> {
    let mut broker = Broker::new();
    log::info!("Main server loop starting up");

    loop {
        tokio::select! {
            Some(event) = events.next() => {
                broker.handle_event(event).await?;
            }
            Some(shutdown) = shutdown_recv.recv() => if shutdown { break },
            else => break,
        }
    }

    log::info!("Main server loop shutting down");
    Ok(())
}
