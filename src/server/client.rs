use crate::server::broker::{Event, Receiver, Sender};
use crate::server::messages::login_client::{LoginClientMessage, IdentClientMessage};
use crate::server::messages::login_server::{IdentServerParams, LoginServerMessage};
use crate::server::messages::{ClientMessage, SendMessage, ServerMessage};
use anyhow::Result;
use async_std::io::{BufReader, ErrorKind};
use async_std::net::TcpStream;
use async_std::task;
use futures::channel::mpsc;
use futures::{AsyncReadExt, AsyncWrite, AsyncWriteExt, SinkExt, StreamExt};
use uuid::Uuid;
use async_std::io::Read;
use crate::server::client::LoginStatus::LoggedIn;
use LoginStatus::{Greeted, Connected};

#[derive(Debug)]
enum LoginStatus {
    Connected {
        send: Sender<ServerMessage>,
    },
    Greeted {
        send: Sender<ServerMessage>,
        game_version: Uuid,
    },
    LoggedIn,
}

pub async fn client_handler(stream: TcpStream, mut broker: Sender<Event>) -> Result<()> {
    let (client_sender, client_receiver) = mpsc::unbounded();
    let client_id = Uuid::new_v4();
    let writer_handle = task::spawn(client_write_loop(client_id, stream.clone(), client_receiver));
    let mut login_status = Connected { send: client_sender };

    let mut reader = BufReader::new(&stream);
    let mut received = Vec::with_capacity(1024);

    log::info!("Starting handler for new client with id {}", client_id);

    loop {
        if !read_from_client(client_id, &mut reader, &mut received).await {
            break;
        }
        login_status = process_messages(client_id, &mut received, &mut broker, login_status).await?;
    }
    log::info!("Client handler finished for client {}", client_id);
    drop(broker);
    writer_handle.await
}

async fn process_messages(client_id: Uuid, received: &mut Vec<u8>, broker: &mut Sender<Event>, mut login_status: LoginStatus) -> Result<LoginStatus> {
    while received.len() > 0 {
        login_status = match login_status {
            Connected { mut send } => match IdentClientMessage::try_parse(received)? {
                Some(ident) => {
                    send.send(ServerMessage::Login(LoginServerMessage::Ident(IdentServerParams {}))).await?;
                    Greeted { send, game_version: ident.game_version }
                },
                None => return Ok(Connected { send })
            },
            Greeted {send, game_version} => match LoginClientMessage::try_parse(received)?{
                Some(login) => {
                    broker.send(Event::NewClient { id: client_id, send, username: login.username }).await?;
                    LoggedIn
                },
                None => return Ok(Greeted { send, game_version })
            },
            LoggedIn => {
                match ClientMessage::try_parse(received)? {
                    Some(_msg) => continue,
                    None => break,
                }
            },
        }
    }

    Ok(login_status)
}

async fn read_from_client(client_id: Uuid, reader: &mut (impl Read + Unpin), received: &mut Vec<u8>) -> bool {
    let mut read_buf = [0u8; 256];
    let num_read = match reader.read(&mut read_buf).await {
        Ok(0) => {
            log::info!("Client {} closed the connection", client_id);
            return false
        },
        Ok(n) => n,
        Err(e) if e.kind() == ErrorKind::Interrupted || e.kind() == ErrorKind::WouldBlock => return true,
        Err(e) => {
            log::warn!("Error when reading from client {}: {}", client_id, e);
            return false
        }
    };
    received.extend_from_slice(&read_buf[..num_read]);
    true
}

async fn client_write_loop(
    client_id: Uuid,
    mut stream: TcpStream,
    mut messages: Receiver<ServerMessage>,
) -> Result<()> {
    while let Some(msg) = messages.next().await {
        log::debug!("Sending message to client {}: {:?}", client_id, msg);
        match msg {
            ServerMessage::Login(login_msg) => send_message(login_msg, &mut stream).await?,
            ServerMessage::Disconnect => {
                stream.close().await?;
                break;
            }
        }
    }
    Ok(())
}

async fn send_message(
    message: LoginServerMessage,
    writer: &mut (impl AsyncWrite + Unpin),
) -> Result<()> {
    let bytes = message.prepare_message()?;
    writer.write_all(&bytes).await?;
    Ok(())
}
