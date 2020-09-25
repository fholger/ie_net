use crate::broker::GameStatus::{Open, Requested};
use crate::messages::client_command::ClientCommand;
use crate::messages::login_server::{RejectServerMessage, WelcomeServerMessage};
use crate::messages::server_messages::{
    CreateGameMessage, DropChannelMessage, ErrorMessage, JoinChannelMessage, NewChannelMessage,
    NewGameMessage, NewUserMessage, SendMessage, UserJoinedMessage, UserLeftMessage,
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
    users: HashMap<String, Uuid>,
    channels: HashMap<String, Channel>,
    games: HashMap<String, Game>,
}

#[derive(Debug)]
pub enum Event {
    NewClient {
        id: Uuid,
        username: String,
        game_version: Uuid,
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

#[derive(PartialEq, Clone)]
pub enum Location {
    Channel { name: String },
    Game { name: String },
    Nowhere,
}

impl Location {
    pub fn to_string(&self) -> String {
        match self {
            Self::Channel { name } => format!("#{}", name).to_string(),
            Self::Game { name } => format!("${}", name).to_string(),
            Self::Nowhere => "[nowhere]".to_string(),
        }
    }
}

#[derive(Clone)]
struct Client {
    id: Uuid,
    username: String,
    location: Location,
    game_version: Uuid,
    send: Sender<Arc<dyn ServerMessage>>,
}

struct Channel {
    name: String,
}

#[derive(PartialEq)]
enum GameStatus {
    Requested,
    Open,
    Started,
}

struct Game {
    hosted_by: Uuid,
    id: Uuid,
    name: String,
    password: Vec<u8>,
    status: GameStatus,
}

const DEFAULT_CHANNEL: &str = "General";
const ALLOWED_CHANNEL_CHARS: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";

fn contains_only_allowed_chars(input: &str, allowed: &str) -> bool {
    input.chars().all(|c| allowed.contains(c))
}

impl Broker {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
            users: HashMap::new(),
            channels: HashMap::new(),
            games: HashMap::new(),
        }
    }

    async fn send(client: &mut Client, message: Arc<dyn ServerMessage>) {
        if let Err(_) = client.send.send(message).await {
            log::error!("Failed to send message to client {}", client.id);
        }
    }

    async fn send_all(clients: &mut HashMap<Uuid, Client>, message: Arc<dyn ServerMessage>) {
        for client in clients.values_mut() {
            Self::send(client, message.clone()).await;
        }
    }

    async fn send_to_location(
        clients: &mut HashMap<Uuid, Client>,
        location: Location,
        message: Arc<dyn ServerMessage>,
    ) {
        for client in clients.values_mut() {
            if client.location == location {
                Self::send(client, message.clone()).await;
            }
        }
    }

    async fn add_channel(&mut self, channel_name: String) {
        if let Entry::Vacant(entry) = self.channels.entry(channel_name.clone()) {
            log::info!("Adding new channel {}", channel_name);
            entry.insert(Channel {
                name: channel_name.clone(),
            });
            Self::send_all(
                &mut self.clients,
                Arc::new(NewChannelMessage { channel_name }),
            )
            .await;
        }
    }

    async fn check_remove_location(&mut self, location: Location) {
        let in_location = self
            .clients
            .values()
            .filter(|c| c.location == location)
            .count();
        if in_location != 0 {
            log::debug!(
                "{} users remain in location {}",
                in_location,
                location.to_string()
            );
            return;
        }
        match location {
            Location::Channel { name } => {
                log::info!("Removing channel {}", name);
                self.channels.remove(&name);
                Self::send_all(
                    &mut self.clients,
                    Arc::new(DropChannelMessage { channel_name: name }),
                )
                .await;
            }
            _ => (),
        }
    }

    async fn message_channel(&mut self, client: Client, message: Vec<u8>) {
        let send_msg = Arc::new(SendMessage {
            username: client.username,
            message,
        });
        Self::send_to_location(&mut self.clients, client.location.clone(), send_msg).await;
    }

    async fn join_channel(&mut self, mut client: Client, channel: String) {
        if !contains_only_allowed_chars(&channel, ALLOWED_CHANNEL_CHARS) {
            Self::send(
                &mut client,
                Arc::new(ErrorMessage {
                    error: "Invalid channel name".to_string(),
                }),
            )
            .await;
            return;
        }

        let prev_location = client.location.clone();
        let origin = None;

        self.add_channel(channel.clone()).await;
        // send join message and list of users in new channel
        Self::send(
            &mut client,
            Arc::new(JoinChannelMessage {
                channel_name: channel.clone(),
            }),
        )
        .await;
        // inform users in channel about the join
        Self::send_to_location(
            &mut self.clients,
            Location::Channel {
                name: channel.clone(),
            },
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
        Self::send_to_location(
            &mut self.clients,
            prev_location.clone(),
            Arc::new(UserLeftMessage {
                username: client.username.clone(),
                destination: Some(destination),
            }),
        )
        .await;

        // update channel information for client
        client.location = Location::Channel {
            name: channel.clone(),
        };
        self.clients.insert(client.id, client);

        self.check_remove_location(prev_location).await;
    }

    async fn host_game(
        &mut self,
        mut client: Client,
        game_name: String,
        password_or_guid: Vec<u8>,
    ) {
        let games = &mut self.games;
        match games.entry(game_name.to_ascii_lowercase()) {
            Entry::Vacant(e) => {
                log::info!(
                    "Client {} is requesting to host new game {}",
                    client.id,
                    game_name
                );
                let message = Arc::new(CreateGameMessage {
                    game_name: game_name.clone(),
                    password: password_or_guid.clone(),
                    version: client.game_version.clone(),
                    id: Uuid::new_v4(),
                });
                Self::send(&mut client, message).await;
                e.insert(Game {
                    hosted_by: client.id.clone(),
                    name: game_name,
                    password: password_or_guid,
                    status: Requested,
                    id: Uuid::from_u128(0),
                });
            }
            Entry::Occupied(mut e) => {
                let maybe_guid = Uuid::parse_str(&String::from_utf8_lossy(&password_or_guid));
                if e.get().status != Requested
                    || e.get().hosted_by != client.id
                    || maybe_guid.is_err()
                {
                    Self::send(
                        &mut client,
                        Arc::new(ErrorMessage {
                            error: "Game already exists.".to_string(),
                        }),
                    )
                    .await;
                    return;
                }
                let guid = maybe_guid.unwrap();
                log::info!(
                    "Client {} is hosting game {} with uuid {}",
                    client.id,
                    game_name,
                    guid
                );
                e.get_mut().id = guid;
                e.get_mut().status = Open;
                Self::send_all(
                    &mut self.clients,
                    Arc::new(NewGameMessage {
                        game_name,
                        id: e.get().id.clone(),
                    }),
                )
                .await;
                let prev_location = client.location;
                let destination = prev_location.to_string();
                client.location = Location::Game {
                    name: e.get().name.clone(),
                };
                self.clients.remove(&client.id);
                Self::send_to_location(
                    &mut self.clients,
                    prev_location,
                    Arc::new(UserLeftMessage {
                        username: client.username.clone(),
                        destination: Some(destination),
                    }),
                )
                .await;
                self.clients.insert(client.id.clone(), client);
            }
        }
    }

    async fn handle_client_command(&mut self, id: Uuid, command: ClientCommand) {
        let mut client = match self.clients.get(&id) {
            Some(client) => client.clone(),
            None => {
                log::info!("Received message for {}, but client does not exist", id);
                return;
            }
        };
        match command {
            ClientCommand::Send { message } => self.message_channel(client, message).await,
            ClientCommand::Join { channel } => self.join_channel(client, channel).await,
            ClientCommand::HostGame {
                game_name,
                password_or_guid: password,
            } => self.host_game(client, game_name, password).await,
            ClientCommand::Malformed { reason } => {
                Self::send(&mut client, Arc::new(ErrorMessage { error: reason })).await
            }
            ClientCommand::Unknown { command } => {
                Self::send(
                    &mut client,
                    Arc::new(ErrorMessage {
                        error: format!("Unknown command: {}", command),
                    }),
                )
                .await
            }
        }
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        log::info!("New event");
        match event {
            Event::NewClient {
                id,
                username,
                game_version,
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
                            location: Location::Nowhere,
                            game_version,
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
