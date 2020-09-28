mod channel;
mod user;

use crate::broker::channel::Channels;
use crate::broker::user::Users;
use crate::broker::GameStatus::{Open, Requested};
use crate::messages::client_command::ClientCommand;
use crate::messages::login_server::{RejectServerMessage, WelcomeServerMessage};
use crate::messages::server_messages::{
    CreateGameMessage, DropGameMessage, ErrorMessage, JoinChannelMessage, JoinGameMessage,
    NewGameMessage, PrivateMessage, SendMessage, SentPrivateMessage,
};
use crate::messages::ServerMessage;
use crate::util::{bytevec_to_str, only_allowed_chars_not_empty};
use anyhow::Result;
use channel::DEFAULT_CHANNEL;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use user::{Location, User};
use uuid::Uuid;
use GameStatus::Started;

pub type ArcServerMessage = Arc<dyn ServerMessage>;
pub type MessageSender = mpsc::Sender<ArcServerMessage>;
pub type MessageReceiver = mpsc::Receiver<ArcServerMessage>;
pub type EventSender = mpsc::Sender<Event>;
pub type EventReceiver = mpsc::Receiver<Event>;

struct Broker {
    users: Users,
    channels: Channels,
    games: HashMap<String, Game>,
}

#[derive(Debug)]
pub enum Event {
    NewUser {
        id: Uuid,
        username: String,
        game_version: Uuid,
        ip_addr: Ipv4Addr,
        send: MessageSender,
    },
    Command {
        id: Uuid,
        command: ClientCommand,
    },
    DropClient {
        id: Uuid,
    },
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

const ALLOWED_NAME_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";

impl Broker {
    fn new() -> Self {
        Self {
            users: Users::new(),
            channels: Channels::new(),
            games: HashMap::new(),
        }
    }

    async fn public_message(&mut self, user: User, message: Vec<u8>) {
        let send_msg = Arc::new(SendMessage {
            username: user.username,
            message,
        });
        self.users
            .send_to_location(user.location.clone(), send_msg)
            .await;
    }

    async fn private_message(&mut self, mut user: User, target: String, message: Vec<u8>) {
        match &target[0..1] {
            "#" => {
                if let Some(channel) = self.channels.get(&target[1..]) {
                    user.send(Arc::new(SentPrivateMessage {
                        to: format!("#{}", channel.name),
                        message: message.clone(),
                    }))
                    .await;
                    self.users
                        .send_to_location(
                            channel.to_location(),
                            Arc::new(PrivateMessage {
                                from: user.username.clone(),
                                to: format!("#{}", channel.name),
                                location: user.location.to_string(),
                                message,
                            }),
                        )
                        .await;
                    return;
                }
            }
            "$" => {
                if let Some(game) = self.games.get(&target[1..].to_ascii_lowercase()) {
                    user.send(Arc::new(SentPrivateMessage {
                        to: format!("${}", game.name),
                        message: message.clone(),
                    }))
                    .await;
                    self.users
                        .send_to_location(
                            Location::Game {
                                name: game.name.clone(),
                            },
                            Arc::new(PrivateMessage {
                                from: user.username.clone(),
                                to: format!("${}", game.name),
                                location: user.location.to_string(),
                                message,
                            }),
                        )
                        .await;
                    return;
                }
            }
            _ => {
                if let Some(recipient) = self.users.by_username_mut(&target) {
                    user.send(Arc::new(SentPrivateMessage {
                        to: recipient.username.clone(),
                        message: message.clone(),
                    }))
                    .await;
                    recipient
                        .send(Arc::new(PrivateMessage {
                            from: user.username.clone(),
                            to: recipient.username.clone(),
                            location: user.location.to_string(),
                            message,
                        }))
                        .await;
                    return;
                }
            }
        }

        user.send(Arc::new(ErrorMessage {
            error: format!("Unknown target: {}", target).to_string(),
        }))
        .await;
    }

    async fn join_channel(&mut self, mut user: User, channel_name: String) {
        if !only_allowed_chars_not_empty(&channel_name, ALLOWED_NAME_CHARS) {
            user.send(Arc::new(ErrorMessage {
                error: "Invalid channel name".to_string(),
            }))
            .await;
            return;
        }

        let channel = self
            .channels
            .get_or_create(&mut self.users, &channel_name)
            .await;
        if channel.to_location() == user.location {
            log::debug!("User is already in requested channel, nothing to do");
            return;
        }

        // send join message and list of users in new channel
        user.send(Arc::new(JoinChannelMessage {
            channel_name: channel.name.clone(),
        }))
        .await;
        for u in self.users.users_in_location(&channel.to_location()) {
            user.send(u.to_new_user_message()).await;
        }

        // update channel information for client
        user.location = channel.to_location();
        self.users.update(user).await;
    }

    async fn host_game(&mut self, mut user: User, game_name: String, password_or_guid: Vec<u8>) {
        if !only_allowed_chars_not_empty(&game_name, ALLOWED_NAME_CHARS) {
            user.send(Arc::new(ErrorMessage {
                error: "Invalid game name".to_string(),
            }))
            .await;
            return;
        }

        match self.games.entry(game_name.to_ascii_lowercase()) {
            Entry::Vacant(e) => {
                log::info!(
                    "Client {} is requesting to host new game {}",
                    user.id,
                    game_name
                );
                user.send(Arc::new(CreateGameMessage {
                    game_name: game_name.clone(),
                    password: password_or_guid.clone(),
                    version: user.game_version.clone(),
                    id: Uuid::new_v4(),
                }))
                .await;
                e.insert(Game {
                    hosted_by: user.id.clone(),
                    host_ip: user.ip_addr.clone(),
                    name: game_name,
                    password: password_or_guid,
                    status: Requested,
                    id: Uuid::from_u128(0),
                });
            }
            Entry::Occupied(mut e) => {
                let maybe_guid = Uuid::parse_str(&String::from_utf8_lossy(&password_or_guid));
                if e.get().status == Started || e.get().hosted_by != user.id || maybe_guid.is_err()
                {
                    user.send(Arc::new(ErrorMessage {
                        error: "Game already exists.".to_string(),
                    }))
                    .await;
                    return;
                }
                let guid = maybe_guid.unwrap();
                match e.get().status {
                    Requested => {
                        log::info!(
                            "Client {} is hosting game {} with uuid {}",
                            user.id,
                            game_name,
                            guid
                        );
                        e.get_mut().id = guid;
                        e.get_mut().status = Open;
                        self.users
                            .send_to_all(Arc::new(NewGameMessage {
                                game_name,
                                id: e.get().id.clone(),
                            }))
                            .await;
                        user.location = Location::Game {
                            name: e.get().name.clone(),
                        };
                        self.users.update(user).await;
                    }
                    Open => {
                        log::info!("Game {} has been started", e.get().name);
                        e.get_mut().status = Started;
                        self.users
                            .send_to_all(Arc::new(DropGameMessage {
                                game_name: e.get().name.clone(),
                            }))
                            .await;
                    }
                    Started => (),
                }
            }
        }
    }

    async fn join_game(&mut self, mut user: User, game_name: String, password: Vec<u8>) {
        if let Some(game) = self.games.get(&game_name.to_ascii_lowercase()) {
            let game_version = user.game_version.clone();
            if let Ok(id) = Uuid::parse_str(&bytevec_to_str(&password)) {
                if id == game.id {
                    log::info!("Client {} has joined game {}", user.id, game.name);
                    user.location = Location::Game {
                        name: game.name.clone(),
                    };
                    self.users.update(user).await;
                }
            } else {
                if password == game.password {
                    user.send(Arc::new(JoinGameMessage {
                        version: game_version,
                        game_name: game.name.clone(),
                        password,
                        id: game.id.clone(),
                        ip_addr: game.host_ip.clone(),
                    }))
                    .await;
                } else {
                    user.send(Arc::new(ErrorMessage {
                        error: "Invalid password".to_string(),
                    }))
                    .await;
                }
            }
        } else {
            user.send(Arc::new(ErrorMessage {
                error: "Game does not exist".to_string(),
            }))
            .await;
        }
    }

    async fn handle_client_command(&mut self, id: Uuid, command: ClientCommand) {
        let mut user = match self.users.by_user_id(&id) {
            Some(user) => user.clone(),
            None => {
                log::info!("Received message for {}, but client does not exist", id);
                return;
            }
        };
        match command {
            ClientCommand::Send { message } => self.public_message(user, message).await,
            ClientCommand::PrivateMessage { target, message } => {
                self.private_message(user, target, message).await
            }
            ClientCommand::Join { channel } => self.join_channel(user, channel).await,
            ClientCommand::HostGame {
                game_name,
                password_or_guid,
            } => self.host_game(user, game_name, password_or_guid).await,
            ClientCommand::JoinGame {
                game_name,
                password,
            } => self.join_game(user, game_name, password).await,
            ClientCommand::NoOp => (),
            ClientCommand::Malformed { reason } => {
                user.send(Arc::new(ErrorMessage { error: reason })).await
            }
            ClientCommand::Unknown { command } => {
                user.send(Arc::new(ErrorMessage {
                    error: format!("Unknown command: {}", command),
                }))
                .await
            }
        }
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::NewUser {
                id,
                username,
                game_version,
                ip_addr,
                send,
            } => {
                self.handle_new_user(id, username, game_version, ip_addr, send)
                    .await
            }
            Event::Command { id, command } => self.handle_client_command(id, command).await,
            Event::DropClient { id } => {
                log::info!("Client {} disconnected, dropping", id);
                self.users.remove(id).await;
            }
        }

        self.channels
            .check_remove_empty_channels(&mut self.users)
            .await;
        Ok(())
    }

    async fn handle_new_user(
        &mut self,
        id: Uuid,
        username: String,
        game_version: Uuid,
        ip_addr: Ipv4Addr,
        send: MessageSender,
    ) {
        let mut user = User {
            id,
            username,
            location: Location::Nowhere,
            game_version,
            ip_addr,
            send,
        };

        if let Some(_) = self.users.by_username(&user.username) {
            log::info!(
                "A client with username {} is already logged in, dropping client",
                user.username
            );
            user.send(Arc::new(RejectServerMessage {
                reason: "Already logged in".to_string(),
            }))
            .await;
            return;
        }

        log::info!(
            "User {} has successfully logged in as {}",
            user.id,
            user.username
        );
        user.send(Arc::new(WelcomeServerMessage {
            server_ident: "IE::Net".to_string(),
            welcome_message: "Welcome to IE::Net, a community-operated EarthNet server".to_string(),
            players_total: 0,
            players_online: 0,
            channels_total: 0,
            games_total: 0,
            games_running: 0,
            games_available: 0,
            game_versions: vec!["tmp2.2".to_string()],
            initial_channel: DEFAULT_CHANNEL.to_string(),
        }))
        .await;

        // send list of channels
        self.channels.announce_all(&mut user).await;
        // send list of open games
        for game in self.games.values() {
            if game.status == Open {
                user.send(Arc::new(NewGameMessage {
                    id: game.id,
                    game_name: game.name.clone(),
                }))
                .await;
            }
        }

        self.users.insert(user).await;
        self.join_channel(
            self.users.by_user_id(&id).unwrap().clone(),
            DEFAULT_CHANNEL.to_string(),
        )
        .await;
    }
}

pub async fn broker_loop(
    mut events: EventReceiver,
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
