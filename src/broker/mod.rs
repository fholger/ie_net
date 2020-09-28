mod channel;
mod game;
mod user;

use crate::broker::channel::Channels;
use crate::broker::game::Games;
use crate::broker::user::Users;
use crate::messages::client_command::ClientCommand;
use crate::messages::login_server::WelcomeServerMessage;
use crate::messages::server_messages::{
    ErrorMessage, JoinChannelMessage, JoinGameMessage, PrivateMessage, SendMessage,
    SentPrivateMessage,
};
use crate::messages::ServerMessage;
use crate::util::{bytevec_to_str, only_allowed_chars_not_empty};
use anyhow::Result;
use channel::DEFAULT_CHANNEL;
use game::GameStatus::Requested;
use game::GameStatus::Started;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use user::{Location, User};
use uuid::Uuid;

pub type ArcServerMessage = Arc<dyn ServerMessage>;
pub type MessageSender = mpsc::Sender<ArcServerMessage>;
pub type MessageReceiver = mpsc::Receiver<ArcServerMessage>;
pub type EventSender = mpsc::Sender<Event>;
pub type EventReceiver = mpsc::Receiver<Event>;

struct Broker {
    users: Users,
    channels: Channels,
    games: Games,
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

const ALLOWED_NAME_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";

impl Broker {
    fn new() -> Self {
        Self {
            users: Users::new(),
            channels: Channels::new(),
            games: Games::new(),
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
            user.send(ErrorMessage::new("Invalid game name")).await;
            return;
        }

        if let Some(game) = self.games.get(&game_name) {
            let maybe_guid = Uuid::parse_str(&String::from_utf8_lossy(&password_or_guid));
            if game.status == Started || game.hosted_by != user.id || maybe_guid.is_err() {
                user.send(ErrorMessage::new("Game already exists.")).await;
                return;
            }
            let status = game.status;
            if status == Requested {
                user.location = game.to_location();
                self.games
                    .open_game(&mut self.users, &game_name, maybe_guid.unwrap())
                    .await;
                self.users.update(user).await;
            } else {
                self.games.start_game(&mut self.users, &game_name).await;
            }
        } else {
            self.games
                .create_game(&mut user, &game_name, &password_or_guid)
                .await;
        }
    }

    async fn join_game(&mut self, mut user: User, game_name: String, password: Vec<u8>) {
        if let Some(game) = self.games.get(&game_name) {
            let game_version = user.game_version.clone();
            if let Ok(id) = Uuid::parse_str(&bytevec_to_str(&password)) {
                if id == game.id {
                    log::info!("Client {} has joined game {}", user.id, game.name);
                    user.location = game.to_location();
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

        self.channels.announce_all(&mut user).await;
        self.games.announce_open(&mut user).await;

        self.users.insert(user).await;
        self.join_channel(
            self.users.by_user_id(&id).unwrap().clone(),
            DEFAULT_CHANNEL.to_string(),
        )
        .await;
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
        self.games.check_remove_empty_games(&mut self.users).await;
        Ok(())
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
