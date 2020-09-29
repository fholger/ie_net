use anyhow::Result;
use downcast_rs::__std::collections::HashSet;
use ie_net::broker::user::Location;
use ie_net::broker::{broker_loop, Event, EventSender, MessageReceiver};
use ie_net::messages::client_command::ClientCommand;
use ie_net::messages::server_messages::{
    DropChannelMessage, DropGameMessage, JoinChannelMessage, NewChannelMessage, NewGameMessage,
    NewUserMessage, UserJoinedMessage, UserLeftMessage,
};
use std::net::Ipv4Addr;
use tokio::sync::{mpsc, watch};
use tokio::task;
use tokio::task::JoinHandle;
use uuid::Uuid;

pub struct TestBroker {
    events: EventSender,
    shutdown_send: watch::Sender<bool>,
    join_handle: JoinHandle<Result<()>>,
}

pub struct TestClient {
    id: Uuid,
    messages: MessageReceiver,
    channels: HashSet<String>,
    games: HashSet<String>,
    users: HashSet<String>,
    location: Location,
}

impl TestBroker {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(64);
        let (shutdown_send, shutdown_recv) = watch::channel(false);
        let join_handle = task::spawn(broker_loop(receiver, shutdown_recv));
        Self {
            events: sender,
            shutdown_send,
            join_handle,
        }
    }

    pub async fn new_client(&mut self, username: &str) -> TestClient {
        let id = Uuid::new_v4();
        let (message_send, message_recv) = mpsc::channel(256);
        self.send(Event::NewUser {
            send: message_send,
            id,
            ip_addr: Ipv4Addr::new(127, 0, 0, 1),
            username: username.to_string(),
            game_version: Uuid::parse_str("534ba248-a87c-4ce9-8bee-bc376aae6134").unwrap(),
        })
        .await;

        TestClient {
            id,
            messages: message_recv,
            users: HashSet::new(),
            channels: HashSet::new(),
            games: HashSet::new(),
            location: Location::Nowhere,
        }
    }

    pub async fn shutdown(self) {
        drop(self.events);
        //self.shutdown_send.broadcast(true).unwrap();
        self.join_handle.await.unwrap().unwrap();
    }

    pub async fn send(&mut self, event: Event) {
        self.events.send(event).await.unwrap();
    }

    pub async fn send_command(&mut self, client: &TestClient, command: ClientCommand) {
        self.send(Event::Command {
            id: client.id,
            command,
        })
        .await;
    }
}

impl TestClient {
    pub async fn process_messages(&mut self) {
        while let Some(message) = self.messages.recv().await {
            if let Some(join) = message.downcast_ref::<JoinChannelMessage>() {
                self.location = Location::Channel {
                    name: join.channel_name.clone(),
                };
                self.users.clear();
            }
            if let Some(newuser) = message.downcast_ref::<NewUserMessage>() {
                self.users.insert(newuser.username.clone());
            }
            if let Some(newuser) = message.downcast_ref::<UserJoinedMessage>() {
                self.users.insert(newuser.username.clone());
            }
            if let Some(dropuser) = message.downcast_ref::<UserLeftMessage>() {
                self.users.remove(&dropuser.username);
            }
            if let Some(newchannel) = message.downcast_ref::<NewChannelMessage>() {
                self.channels.insert(newchannel.channel_name.clone());
            }
            if let Some(dropchannel) = message.downcast_ref::<DropChannelMessage>() {
                self.channels.remove(&dropchannel.channel_name);
            }
            if let Some(newgame) = message.downcast_ref::<NewGameMessage>() {
                self.games.insert(newgame.game_name.clone());
            }
            if let Some(dropgame) = message.downcast_ref::<DropGameMessage>() {
                self.games.remove(&dropgame.game_name);
            }
        }
    }

    pub fn should_have_channel(&self, channel: &str) {
        assert!(self.channels.contains(channel), "missing expected channel");
    }

    pub fn should_not_have_channel(&self, channel: &str) {
        assert!(!self.channels.contains(channel), "unexpected channel");
    }

    pub fn should_be_in(&self, location: &Location) {
        assert_eq!(self.location, *location, "not in expected location");
    }
}
