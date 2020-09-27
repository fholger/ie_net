use uuid::Uuid;
use std::net::Ipv4Addr;
use std::sync::Arc;
use crate::broker::{MessageSender, ArcServerMessage};
use nom::lib::std::collections::HashMap;
use crate::messages::server_messages::{UserJoinedMessage, UserLeftMessage};

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
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub location: Location,
    pub game_version: Uuid,
    pub ip_addr: Ipv4Addr,
    pub send: MessageSender,
}

impl User {
    pub async fn send(&mut self, message: ArcServerMessage) {
        if let Err(_) = self.send.send(message).await {
            // if this happens, it means that the user's receiver was closed
            // this should trigger an event being sent to the broker that the
            // client went away, so we'll just log and ignore the error here
            log::warn!("Failed to send message to user {}", self.id);
        }
    }
}

pub struct Users {
    by_id: HashMap<Uuid, User>,
    by_name: HashMap<String, Uuid>,
}

impl Users {
    pub fn new() -> Self {
        Self {
            by_id: HashMap::new(),
            by_name: HashMap::new(),
        }
    }

    pub fn count_in_location(&self, location: &Location) -> usize {
        self.by_id.values().filter(|c| c.location == *location).count()
    }

    pub fn users_in_location(&self, location: &Location) -> Vec<String> {
        self.by_id.values().filter(|u| u.location == *location).map(|u| u.username.clone()).collect()
    }

    pub fn by_username(&self, username: &str) -> Option<&User> {
        if let Some(id) = self.by_name.get(&username.to_ascii_lowercase()) {
            self.by_id.get(id)
        } else {
            None
        }
    }

    pub fn by_username_mut(&mut self, username: &str) -> Option<&mut User> {
        if let Some(id) = self.by_name.get(&username.to_ascii_lowercase()) {
            self.by_id.get_mut(id)
        } else {
            None
        }
    }

    pub fn by_user_id(&self, id: &Uuid) -> Option<&User> {
        self.by_id.get(id)
    }

    pub async fn send_to_all(&mut self, message: ArcServerMessage) {
        for user in self.by_id.values_mut() {
            user.send(message.clone()).await;
        }
    }

    pub async fn send_to_location(
        &mut self,
        location: Location,
        message: ArcServerMessage,
    ) {
        for user in self.by_id.values_mut() {
            if user.location == location {
                user.send(message.clone()).await;
            }
        }
    }

    pub async fn insert(&mut self, user: User) {
        // inform existing users at location of new user
        self.send_to_location(user.location.clone(), Arc::new(UserJoinedMessage {
            username: user.username.clone(),
            origin: None,
            version_idx: 0,
        })).await;

        self.by_name.insert(user.username.to_ascii_lowercase(), user.id.clone());
        self.by_id.insert(user.id.clone(), user);
    }

    pub async fn update(&mut self, user: User) {
        if !self.by_id.contains_key(&user.id) {
            self.insert(user).await;
            return;
        }

        let prev = self.by_id.remove(&user.id).unwrap();
        if prev.location != user.location {
            // inform users at new location of new user
            self.send_to_location(user.location.clone(), Arc::new(UserJoinedMessage {
                username: user.username.clone(),
                origin: Some(prev.location.to_string()),
                version_idx: 0,
            })).await;

            // inform users at previous location of user leaving
            self.send_to_location(prev.location.clone(), Arc::new(UserLeftMessage {
                username: user.username.clone(),
                destination: Some(user.location.to_string()),
            })).await;
        }

        self.by_id.insert(user.id.clone(), user);
    }

    pub async fn remove(&mut self, id: Uuid) {
        if let Some(user) = self.by_id.remove(&id) {
            self.by_name.remove(&user.username.to_ascii_lowercase());
            self.send_to_location(
                user.location,
                Arc::new(UserLeftMessage {
                    username: user.username,
                    destination: None,
                }),
            ).await;
        }
    }
}
