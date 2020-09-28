use crate::broker::user::{Location, User, Users};
use crate::broker::ArcServerMessage;
use crate::messages::server_messages::{DropChannelMessage, NewChannelMessage};
use nom::lib::std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

pub struct Channel {
    pub name: String,
}

pub const DEFAULT_CHANNEL: &str = "General";

impl Channel {
    pub fn to_location(&self) -> Location {
        Location::Channel {
            name: self.name.clone(),
        }
    }

    pub fn to_new_channel_message(&self) -> ArcServerMessage {
        Arc::new(NewChannelMessage {
            channel_name: self.name.clone(),
        })
    }

    pub fn to_drop_channel_message(&self) -> ArcServerMessage {
        Arc::new(DropChannelMessage {
            channel_name: self.name.clone(),
        })
    }
}

pub struct Channels {
    by_name: HashMap<String, Channel>,
}

impl Channels {
    pub fn new() -> Self {
        Channels {
            by_name: HashMap::new(),
        }
    }

    pub async fn get_or_create(&mut self, users: &mut Users, name: &str) -> &Channel {
        if let Entry::Vacant(e) = self.by_name.entry(name.to_ascii_lowercase()) {
            log::info!("Creating new channel {}", name);
            let channel = e.insert(Channel {
                name: name.to_string(),
            });
            users.send_to_all(channel.to_new_channel_message()).await;
        }
        self.get(name).unwrap()
    }

    pub async fn remove(&mut self, users: &mut Users, name: &str) {
        if let Some(channel) = self.by_name.remove(&name.to_ascii_lowercase()) {
            log::info!("Removing channel {}", name);
            users.send_to_all(channel.to_drop_channel_message()).await;
        }
    }

    pub async fn check_remove_empty_channels(&mut self, users: &mut Users) {
        let occupied_locations = users.occupied_locations();
        let empty_channels: Vec<String> = self
            .by_name
            .values()
            .filter(|c| !occupied_locations.contains(&c.to_location()))
            .map(|c| c.name.clone())
            .collect();

        for channel in empty_channels {
            self.remove(users, &channel).await;
        }
    }

    pub fn get(&self, name: &str) -> Option<&Channel> {
        self.by_name.get(&name.to_ascii_lowercase())
    }

    pub async fn announce_all(&mut self, user: &mut User) {
        for channel in self.by_name.values() {
            user.send(channel.to_new_channel_message()).await;
        }
    }
}
