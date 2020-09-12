mod messages;

use anyhow::Result;
use async_std::future::Future;
use async_std::io::{BufReader, Write};
use async_std::net::TcpStream;
use async_std::{net::TcpListener, task};
use futures::{StreamExt, AsyncWriteExt, SinkExt, AsyncReadExt};
use uuid::Uuid;
use messages::login::LoginClientMessage;
use messages::login::{IdentClientParams, LoginClientParams, LoginServerMessage, IdentServerParams, RejectServerParams, WelcomeServerParams};
use messages::{SendMessage, ServerMessage};
use std::collections::HashMap;
use futures::channel::mpsc;
use std::collections::hash_map::Entry;
use crate::server::messages::ClientMessage;

type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

pub fn run(addr: &str) -> Result<()> {
    let accept_task = accept_loop(addr);
    task::block_on(accept_task)
}

async fn accept_loop(addr: &str) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    log::info!("Listening for connections at {}", addr);

    let (main_sender, main_receiver) = mpsc::unbounded();
    let _main_handle = task::spawn(server_loop(main_receiver));

    let mut incoming_connections = listener.incoming();
    while let Some(connection) = incoming_connections.next().await {
        let connection = connection?;
        log::info!("New connection from {}", connection.peer_addr()?);
        spawn_and_log_error(client_login_handler(connection, main_sender.clone()));
    }

    Ok(())
}

pub enum Event {
    NewClient {
        uuid: Uuid,
        messages: Sender<ServerMessage>,
    },
}

async fn server_loop(mut events: Receiver<Event>) -> Result<()> {
    let mut clients: HashMap<Uuid, Sender<ServerMessage>> = HashMap::new();
    log::info!("Main server loop starting up");

    while let Some(event) = events.next().await {
        log::info!("New event");
        match event {
            Event::NewClient { uuid, mut messages } => {
                match clients.entry(uuid) {
                    Entry::Occupied(..) => {
                        // FIXME: actually need to check username, not id
                        messages.send(ServerMessage::Login(LoginServerMessage::Reject(RejectServerParams{ reason: "Already logged in".to_string() }))).await?;
                        messages.send(ServerMessage::Disconnect).await?;
                    }
                    Entry::Vacant(entry) => {
                        log::info!("Client {} has successfully logged in", uuid);
                        messages.send(ServerMessage::Login(LoginServerMessage::Welcome(WelcomeServerParams {
                            server_ident: "IE::Net".to_string(),
                            welcome_message: "Welcome to IE::Net, a community-operated EarthNet server".to_string(),
                            players_total: 25,
                            players_online: 12,
                            channels_total: 1,
                            games_total: 0,
                            games_running: 0,
                            games_available: 0,
                            game_versions: vec!["tdm2.1".to_string()],
                            initial_channel: "General".to_string(),
                        }))).await?;
                        entry.insert(messages);
                    }
                }
            }
        }
    }

    log::info!("Main server loop shutting down");
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ClientStatus {
    Connected,
    Greeted,
    LoggedIn,
}

async fn client_login_handler(stream: TcpStream, mut broker: Sender<Event>) -> Result<()> {
    let client_id = Uuid::new_v4();
    let mut client_status = ClientStatus::Connected;
    let mut reader = BufReader::new(&stream);
    let mut read_buf = [0u8; 1024];
    let mut received = Vec::with_capacity(1024);

    let mut write_stream = stream.clone();

    log::info!("Starting login for new client with id {}", client_id);

    loop {
        let num_read = reader.read(&mut read_buf).await?;
        if num_read == 0 {
            log::info!("Client {} closed the connection", client_id);
            break;
        }
        received.extend_from_slice(&read_buf[..num_read]);

        client_status = match LoginClientMessage::try_parse(&mut received, client_status)? {
            Some(LoginClientMessage::Ident(params)) => handle_ident(client_id,params, &mut write_stream).await?,
            Some(LoginClientMessage::Login(params)) => handle_login(client_id, params, &mut write_stream).await?,
            None => continue,
        };
        log::debug!("New client status is {:?}", client_status);

        if client_status == ClientStatus::LoggedIn {
            log::info!("Login for client {} done, handing off...", client_id);
            let (writer_sender, writer_receiver) = mpsc::unbounded();
            broker.send(Event::NewClient { uuid: client_id, messages: writer_sender }).await?;
            spawn_and_log_error(client_write_loop(stream.clone(), writer_receiver));
            spawn_and_log_error(client_read_loop(client_id, stream, broker));
            return Ok(());
        }
    }
    log::debug!("Client login handler finished for client {}", client_id);
    Ok(())
}

async fn client_read_loop(id: Uuid, stream: TcpStream, mut broker: Sender<Event>) -> Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut read_buf = [0u8; 1024];
    let mut received = Vec::with_capacity(1024);
    loop {
        let num_read = reader.read(&mut read_buf).await?;
        if num_read == 0 {
            log::info!("Client {} closed the connection", id);
            break;
        }
        received.extend_from_slice(&read_buf[..num_read]);
        if let Some(message) = ClientMessage::try_parse(&mut received)? {}
    }
    Ok(())
}

async fn client_write_loop(mut stream: TcpStream, mut messages: Receiver<ServerMessage>) -> Result<()> {
    while let Some(msg) = messages.next().await {
        log::debug!("Sending message to client {}: {:?}", "0", msg);
        match msg {
            ServerMessage::Login(login_msg) => send_message(login_msg, &mut stream).await?,
            ServerMessage::Disconnect => {
                stream.close().await?;
                break;
            },
        }
    }
    Ok(())
}

async fn send_message(message: LoginServerMessage, writer: &mut (impl Write + Unpin)) -> Result<()> {
    let bytes = message.prepare_message()?;
    writer.write_all(&bytes).await?;
    Ok(())
}

async fn handle_ident(client_id: Uuid, ident: IdentClientParams, writer: &mut (impl Write + Unpin)) -> Result<ClientStatus> {
    log::debug!("Received ident message from {}: {:?}", client_id, ident);
    // TODO: verify client game version
    let response = LoginServerMessage::Ident(IdentServerParams {});
    send_message(response, writer).await?;
    Ok(ClientStatus::Greeted)
}

async fn handle_login(client_id: Uuid, login: LoginClientParams, _writer: &mut (impl Write + Unpin)) -> Result<ClientStatus> {
    log::debug!("Received login message from {}: {:?}", client_id, login);
    // TODO: verify login
    Ok(ClientStatus::LoggedIn)
}

fn spawn_and_log_error<F>(future: F) -> task::JoinHandle<()>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    task::spawn(async move {
        if let Err(e) = future.await {
            log::error!("Task exited with error: {}", e);
        }
    })
}
