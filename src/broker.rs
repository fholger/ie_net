use crate::broker::GameStatus::{Open, Requested};
use crate::messages::client_command::ClientCommand;
use crate::messages::login_server::{RejectServerMessage, WelcomeServerMessage};
use crate::messages::server_messages::{
    CreateGameMessage, DropChannelMessage, DropGameMessage, ErrorMessage, JoinChannelMessage,
    JoinGameMessage, NewChannelMessage, NewGameMessage, NewUserMessage, PrivateMessage,
    SendMessage, SentPrivateMessage, UserJoinedMessage, UserLeftMessage,
};
use crate::messages::ServerMessage;
use crate::util::{bytevec_to_str, only_allowed_chars_not_empty};
use anyhow::Result;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;
use GameStatus::Started;

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
        ip_addr: Ipv4Addr,
        send: Sender<Arc<dyn ServerMessage>>,
    },
    Command {
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
    ip_addr: Ipv4Addr,
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
    host_ip: Ipv4Addr,
    id: Uuid,
    name: String,
    password: Vec<u8>,
    status: GameStatus,
}

const DEFAULT_CHANNEL: &str = "General";
const ALLOWED_NAME_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";

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
        if let Entry::Vacant(entry) = self.channels.entry(channel_name.to_ascii_lowercase()) {
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
            Location::Game { name } => {
                log::info!("Removing game {}", name);
                self.games.remove(&name);
                Self::send_all(
                    &mut self.clients,
                    Arc::new(DropGameMessage { game_name: name }),
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

    async fn private_message(&mut self, mut client: Client, target: String, message: Vec<u8>) {
        match &target[0..1] {
            "#" => {
                if let Some(channel) = self.channels.get(&target[1..].to_ascii_lowercase()) {
                    Self::send(
                        &mut client,
                        Arc::new(SentPrivateMessage {
                            to: format!("#{}", channel.name),
                            message: message.clone(),
                        }),
                    )
                    .await;
                    Self::send_to_location(
                        &mut self.clients,
                        Location::Channel {
                            name: channel.name.clone(),
                        },
                        Arc::new(PrivateMessage {
                            from: client.username.clone(),
                            location: client.location.to_string(),
                            message,
                        }),
                    )
                    .await;
                    return;
                }
            }
            "$" => {
                if let Some(game) = self.games.get(&target[1..].to_ascii_lowercase()) {
                    Self::send(
                        &mut client,
                        Arc::new(SentPrivateMessage {
                            to: format!("${}", game.name),
                            message: message.clone(),
                        }),
                    )
                    .await;
                    Self::send_to_location(
                        &mut self.clients,
                        Location::Game {
                            name: game.name.clone(),
                        },
                        Arc::new(PrivateMessage {
                            from: client.username.clone(),
                            location: client.location.to_string(),
                            message,
                        }),
                    )
                    .await;
                    return;
                }
            }
            _ => {
                if let Some(user_id) = self.users.get(&target.to_ascii_lowercase()) {
                    if let Some(user) = self.clients.get_mut(user_id) {
                        Self::send(
                            &mut client,
                            Arc::new(SentPrivateMessage {
                                to: user.username.clone(),
                                message: message.clone(),
                            }),
                        )
                        .await;
                        Self::send(
                            user,
                            Arc::new(PrivateMessage {
                                from: client.username.clone(),
                                location: client.location.to_string(),
                                message,
                            }),
                        )
                        .await;
                        return;
                    }
                }
            }
        }

        Self::send(
            &mut client,
            Arc::new(ErrorMessage {
                error: format!("Unknown target: {}", target).to_string(),
            }),
        )
        .await;
    }

    async fn join_channel(&mut self, mut client: Client, channel: String) {
        if !only_allowed_chars_not_empty(&channel, ALLOWED_NAME_CHARS) {
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
        let channel_name = self
            .channels
            .get(&channel.to_ascii_lowercase())
            .unwrap()
            .name
            .clone();
        drop(channel);
        Self::send(
            &mut client,
            Arc::new(JoinChannelMessage {
                channel_name: channel_name.clone(),
            }),
        )
        .await;
        let new_location = Location::Channel {
            name: channel_name.clone(),
        };
        for user in self.clients.values_mut() {
            if user.location == new_location {
                Self::send(
                    &mut client,
                    Arc::new(NewUserMessage {
                        username: user.username.clone(),
                    }),
                )
                .await;
            }
        }
        // inform users in channel about the join
        Self::send_to_location(
            &mut self.clients,
            Location::Channel {
                name: channel_name.clone(),
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
        let destination = format!("#{}", channel_name);
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
            name: channel_name.clone(),
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
        if !only_allowed_chars_not_empty(&game_name, ALLOWED_NAME_CHARS) {
            Self::send(
                &mut client,
                Arc::new(ErrorMessage {
                    error: "Invalid game name".to_string(),
                }),
            )
            .await;
            return;
        }

        match self.games.entry(game_name.to_ascii_lowercase()) {
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
                    host_ip: client.ip_addr.clone(),
                    name: game_name,
                    password: password_or_guid,
                    status: Requested,
                    id: Uuid::from_u128(0),
                });
            }
            Entry::Occupied(mut e) => {
                let maybe_guid = Uuid::parse_str(&String::from_utf8_lossy(&password_or_guid));
                if e.get().status == Started
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
                match e.get().status {
                    Requested => {
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
                        client.location = Location::Game {
                            name: e.get().name.clone(),
                        };
                        let destination = client.location.to_string();
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
                    Open => {
                        log::info!("Game {} has been started", e.get().name);
                        e.get_mut().status = Started;
                        Self::send_all(
                            &mut self.clients,
                            Arc::new(DropGameMessage {
                                game_name: e.get().name.clone(),
                            }),
                        )
                        .await;
                    }
                    Started => (),
                }
            }
        }
    }

    async fn join_game(&mut self, mut client: Client, game_name: String, password: Vec<u8>) {
        if let Some(game) = self.games.get(&game_name.to_ascii_lowercase()) {
            let game_version = client.game_version.clone();
            if let Ok(id) = Uuid::parse_str(&bytevec_to_str(&password)) {
                if id == game.id {
                    log::info!("Client {} has joined game {}", client.id, game.name);
                    let prev_location = client.location;
                    let username = client.username.clone();
                    client.location = Location::Game {
                        name: game.name.clone(),
                    };
                    self.clients.insert(client.id.clone(), client);
                    Self::send_to_location(
                        &mut self.clients,
                        prev_location,
                        Arc::new(UserLeftMessage {
                            username,
                            destination: Some(format!("${}", game.name).to_string()),
                        }),
                    )
                    .await;
                }
            } else {
                if password == game.password {
                    Self::send(
                        &mut client,
                        Arc::new(JoinGameMessage {
                            version: game_version,
                            game_name: game.name.clone(),
                            password,
                            id: game.id.clone(),
                            ip_addr: game.host_ip.clone(),
                        }),
                    )
                    .await;
                } else {
                    Self::send(
                        &mut client,
                        Arc::new(ErrorMessage {
                            error: "Invalid password".to_string(),
                        }),
                    )
                    .await;
                }
            }
        } else {
            Self::send(
                &mut client,
                Arc::new(ErrorMessage {
                    error: "Game does not exist".to_string(),
                }),
            )
            .await;
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
            ClientCommand::PrivateMessage { target, message } => {
                self.private_message(client, target, message).await
            }
            ClientCommand::Join { channel } => self.join_channel(client, channel).await,
            ClientCommand::HostGame {
                game_name,
                password_or_guid,
            } => self.host_game(client, game_name, password_or_guid).await,
            ClientCommand::JoinGame {
                game_name,
                password,
            } => self.join_game(client, game_name, password).await,
            ClientCommand::NoOp => (),
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
        match event {
            Event::NewClient {
                id,
                username,
                game_version,
                ip_addr,
                send,
            } => {
                self.handle_new_client(id, username, game_version, ip_addr, send)
                    .await
            }
            Event::Command { id, command } => self.handle_client_command(id, command).await,
            Event::DropClient { id } => {
                log::info!("Client {} disconnected, dropping", id);
                if let Some(client) = self.clients.remove(&id) {
                    self.users.remove(&client.username.to_ascii_lowercase());
                    Self::send_to_location(
                        &mut self.clients,
                        client.location,
                        Arc::new(UserLeftMessage {
                            username: client.username,
                            destination: None,
                        }),
                    )
                    .await;
                }
            }
        }
        Ok(())
    }

    async fn handle_new_client(
        &mut self,
        id: Uuid,
        username: String,
        game_version: Uuid,
        ip_addr: Ipv4Addr,
        send: Sender<Arc<dyn ServerMessage>>,
    ) {
        let mut client = Client {
            id,
            username,
            location: Location::Nowhere,
            game_version,
            ip_addr,
            send,
        };

        if self
            .users
            .contains_key(&client.username.to_ascii_lowercase())
        {
            log::info!(
                "A client with username {} is already logged in, dropping client",
                client.username
            );
            Self::send(
                &mut client,
                Arc::new(RejectServerMessage {
                    reason: "Already logged in".to_string(),
                }),
            )
            .await;
            return;
        }

        log::info!("Client {} has successfully logged in", client.id);
        Self::send(
            &mut client,
            Arc::new(WelcomeServerMessage {
                server_ident: "IE::Net".to_string(),
                welcome_message: "Welcome to IE::Net, a community-operated EarthNet server"
                    .to_string(),
                players_total: 0,
                players_online: 0,
                channels_total: 0,
                games_total: 0,
                games_running: 0,
                games_available: 0,
                game_versions: vec!["tdm2.2".to_string()],
                initial_channel: DEFAULT_CHANNEL.to_string(),
            }),
        )
        .await;

        // send list of channels
        for channel in self.channels.values() {
            Self::send(
                &mut client,
                Arc::new(NewChannelMessage {
                    channel_name: channel.name.clone(),
                }),
            )
            .await;
        }
        // send list of open games
        for game in self.games.values() {
            if game.status == Open {
                Self::send(
                    &mut client,
                    Arc::new(NewGameMessage {
                        id: game.id,
                        game_name: game.name.clone(),
                    }),
                )
                .await;
            }
        }

        self.users
            .insert(client.username.to_ascii_lowercase(), client.id.clone());
        self.clients.insert(client.id.clone(), client);
        self.join_channel(
            self.clients.get(&id).unwrap().clone(),
            DEFAULT_CHANNEL.to_string(),
        )
        .await;
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
