use crate::broker::game::GameStatus::{Open, Requested, Started};
use crate::broker::user::{Location, User, Users};
use crate::broker::ArcServerMessage;
use crate::messages::server_messages::{CreateGameMessage, DropGameMessage, NewGameMessage};
use nom::lib::std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::Duration;
use uuid::Uuid;

pub const ALLOWED_GAME_NAME_CHARS: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_+.| ";

#[derive(PartialEq, Clone, Copy)]
pub enum GameStatus {
    Requested,
    Open,
    Started,
}

pub struct Game {
    pub hosted_by: Uuid,
    pub host_ip: Ipv4Addr,
    pub id: Uuid,
    pub game_version: Uuid,
    pub name: String,
    pub password: Vec<u8>,
    pub status: GameStatus,
    pub created_at: Instant,
}

impl Game {
    pub fn to_location(&self) -> Location {
        Location::Game {
            name: self.name.clone(),
        }
    }

    pub fn to_new_game_message(&self) -> ArcServerMessage {
        Arc::new(NewGameMessage {
            id: self.id,
            game_name: self.name.clone(),
        })
    }

    pub fn to_drop_game_message(&self) -> ArcServerMessage {
        Arc::new(DropGameMessage {
            game_name: self.name.clone(),
        })
    }
}

pub struct Games {
    by_name: HashMap<String, Game>,
}

impl Games {
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
        }
    }

    pub fn count(&self) -> u32 {
        self.by_name.len() as u32
    }

    pub fn count_open(&self) -> u32 {
        self.by_name.values().filter(|g| g.status == Open).count() as u32
    }

    pub fn get(&self, name: &str) -> Option<&Game> {
        self.by_name.get(&name.to_ascii_lowercase())
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Game> {
        self.by_name.get_mut(&name.to_ascii_lowercase())
    }

    pub async fn create_game(&mut self, user: &mut User, name: &str, password: &[u8]) {
        log::info!(
            "User {} has requested to host new game {}",
            user.username,
            name
        );
        let game = Game {
            hosted_by: user.id,
            host_ip: user.ip_addr,
            name: name.to_string(),
            password: password.to_vec(),
            status: Requested,
            id: Uuid::from_u128(0),
            game_version: user.game_version,
            created_at: Instant::now(),
        };
        user.send(Arc::new(CreateGameMessage {
            game_name: game.name.clone(),
            password: game.password.clone(),
            version: game.game_version,
            id: Uuid::new_v4(),
        }))
        .await;
        self.by_name.insert(name.to_ascii_lowercase(), game);
    }

    pub async fn open_game(&mut self, users: &mut Users, name: &str, id: Uuid) {
        if let Some(game) = self.get_mut(name) {
            log::info!("Game {} is now open", name);
            game.id = id;
            game.status = Open;
            users.send_to_all(game.to_new_game_message()).await;
        }
    }

    pub async fn start_game(&mut self, users: &mut Users, name: &str) {
        if let Some(game) = self.get_mut(name) {
            log::info!("Game {} has started", name);
            game.status = Started;
            users.send_to_all(game.to_drop_game_message()).await;
        }
    }

    pub async fn remove(&mut self, users: &mut Users, name: &str) {
        if let Some(game) = self.by_name.remove(&name.to_ascii_lowercase()) {
            log::info!("Removing game {}", name);
            if game.status == Open {
                users.send_to_all(game.to_drop_game_message()).await;
            }
        }
    }

    pub async fn check_remove_empty_games(&mut self, users: &mut Users) {
        let occupied_locations = users.occupied_locations();
        let empty_games: Vec<String> = self
            .by_name
            .values()
            .filter(|g| {
                if g.status == Requested {
                    g.created_at.elapsed() > Duration::new(30, 0)
                } else {
                    !occupied_locations.contains(&g.to_location())
                }
            })
            .map(|g| g.name.clone())
            .collect();

        for game in empty_games {
            self.remove(users, &game).await;
        }
    }

    pub async fn announce_open(&self, user: &mut User) {
        for game in self.by_name.values().filter(|g| g.status == Open) {
            user.send(game.to_new_game_message()).await;
        }
    }
}
