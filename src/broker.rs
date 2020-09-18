use crate::messages::client_command::ClientCommand;
use crate::messages::login_server::{RejectServerMessage, WelcomeServerMessage};
use crate::messages::server_messages::{
    DropChannelMessage, ErrorMessage, JoinChannelMessage, NewChannelMessage, NewUserMessage,
    SendMessage, UserJoinedMessage, UserLeftMessage,
};
use crate::messages::ServerMessage;
use anyhow::Result;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

pub type Sender<T> = mpsc::Sender<T>;
pub type Receiver<T> = mpsc::Receiver<T>;

struct Broker {
    clients: HashMap<Uuid, Client>,
    channels: HashMap<String, Channel>,
}

#[derive(Debug)]
pub enum Event {
    NewClient {
        id: Uuid,
        username: String,
        send: Sender<Arc<dyn ServerMessage>>,
    },
    Message {
        id: Uuid,
        command: ClientCommand,
    },
    DropClient {
        id: Uuid,
    },
}

#[derive(Clone)]
struct Client {
    id: Uuid,
    username: String,
    channel: String,
    send: Sender<Arc<dyn ServerMessage>>,
}

struct Channel {
    name: String,
}

const DEFAULT_CHANNEL: &str = "General";
const ALLOWED_CHANNEL_CHARS: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";

async fn send(client: &mut Client, message: Arc<dyn ServerMessage>) {
    if let Err(_) = client.send.send(message).await {
        log::error!("Failed to send message to client {}", client.id);
    }
}

fn contains_only_allowed_chars(input: &str, allowed: &str) -> bool {
    input.chars().all(|c| allowed.contains(c))
}

impl Broker {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
            channels: HashMap::new(),
        }
    }

    async fn send_all(&mut self, message: Arc<dyn ServerMessage>) {
        for client in self.clients.values_mut() {
            send(client, message.clone()).await;
        }
    }

    async fn send_to_channel(&mut self, channel: String, message: Arc<dyn ServerMessage>) {
        for client in self.clients.values_mut() {
            if client.channel == channel {
                send(client, message.clone()).await;
            }
        }
    }

    async fn add_channel(&mut self, channel_name: String) {
        if let Entry::Vacant(entry) = self.channels.entry(channel_name.clone()) {
            log::info!("Adding new channel {}", channel_name);
            entry.insert(Channel {
                name: channel_name.clone(),
            });
            self.send_all(Arc::new(NewChannelMessage { channel_name }))
                .await;
        }
    }

    async fn check_remove_channel(&mut self, channel_name: String) {
        let in_channel = self
            .clients
            .values()
            .filter(|c| c.channel == channel_name)
            .count();
        if in_channel != 0 {
            log::debug!("{} users remain in channel {}", in_channel, channel_name);
            return;
        }
        log::info!("Removing channel {}", channel_name);
        self.channels.remove(&channel_name);
        self.send_all(Arc::new(DropChannelMessage { channel_name }))
            .await;
    }

    async fn message_channel(&mut self, client: Client, message: Vec<u8>) {
        let send_msg = Arc::new(SendMessage {
            username: client.username,
            message,
        });
        self.send_to_channel(client.channel.clone(), send_msg).await;
    }

    async fn join_channel(&mut self, mut client: Client, channel: String) {
        if !contains_only_allowed_chars(&channel, ALLOWED_CHANNEL_CHARS) {
            send(
                &mut client,
                Arc::new(ErrorMessage {
                    error: "Invalid channel name".to_string(),
                }),
            )
            .await;
            return;
        }

        let prev_channel = client.channel.clone();
        let origin = None;

        self.add_channel(channel.clone()).await;
        // send join message and list of users in new channel
        send(
            &mut client,
            Arc::new(JoinChannelMessage {
                channel_name: channel.clone(),
            }),
        )
        .await;
        for user in self.clients.values_mut() {
            if user.channel == channel {
                send(
                    &mut client,
                    Arc::new(NewUserMessage {
                        username: user.username.clone(),
                    }),
                )
                .await;
            }
        }
        // inform users in channel about the join
        self.send_to_channel(
            channel.clone(),
            Arc::new(UserJoinedMessage {
                username: client.username.clone(),
                version_idx: 0,
                origin,
            }),
        )
        .await;

        // inform users from old channel
        self.clients.remove(&client.id);
        let destination = format!("#{}", channel);
        self.send_to_channel(
            prev_channel.clone(),
            Arc::new(UserLeftMessage {
                username: client.username.clone(),
                destination: Some(destination),
            }),
        )
        .await;

        // update channel information for client
        client.channel = channel.clone();
        self.clients.insert(client.id, client);

        self.check_remove_channel(prev_channel).await;
    }

    async fn handle_client_command(&mut self, id: Uuid, command: ClientCommand) {
        let client = match self.clients.get(&id) {
            Some(client) => client.clone(),
            None => {
                log::info!("Received message for {}, but client does not exist", id);
                return;
            }
        };
        match command {
            ClientCommand::Send { message } => self.message_channel(client, message).await,
            ClientCommand::Join { channel } => self.join_channel(client, channel).await,
            ClientCommand::Malformed { reason } => (),
            ClientCommand::Unknown { command } => (),
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
                        send.send(Arc::new(RejectServerMessage {
                            reason: "Already logged in".to_string(),
                        }))
                        .await?;
                    }
                    Entry::Vacant(entry) => {
                        log::info!("Client {} has successfully logged in", id);
                        send.send(Arc::new(WelcomeServerMessage {
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
                            initial_channel: DEFAULT_CHANNEL.to_string(),
                        }))
                        .await?;
                        entry.insert(Client {
                            id,
                            username,
                            channel: "".to_string(),
                            send,
                        });
                        self.join_channel(
                            self.clients.get(&id).unwrap().clone(),
                            DEFAULT_CHANNEL.to_string(),
                        )
                        .await;
                    }
                }
            }
            Event::Message { id, command } => self.handle_client_command(id, command).await,
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
