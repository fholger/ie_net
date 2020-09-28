use crate::broker::{Event, EventSender, MessageReceiver, MessageSender};
use crate::client::LoginStatus::LoggedIn;
use crate::messages::client_command::ClientCommand;
use crate::messages::login_client::{IdentClientMessage, LoginClientMessage};
use crate::messages::login_server::{IdentServerMessage, RejectServerMessage};
use crate::messages::ServerMessage;
use crate::server::spawn_and_log_error;
use crate::util::{bytevec_to_str, only_allowed_chars_not_empty};
use anyhow::Result;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ErrorKind};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::stream::StreamExt;
use tokio::sync::mpsc;
use uuid::Uuid;
use LoginStatus::{Connected, Greeted};

#[derive(Debug)]
enum LoginStatus {
    Connected {
        send: MessageSender,
    },
    Greeted {
        send: MessageSender,
        game_version: Uuid,
    },
    LoggedIn,
}

pub async fn client_handler(stream: TcpStream, mut broker: EventSender) -> Result<()> {
    let ip_addr = match stream.peer_addr()?.ip() {
        IpAddr::V4(ipv4) => ipv4,
        IpAddr::V6(_) => Err(anyhow::anyhow!(
            "IPv6 connections are incompatible with the game"
        ))?,
    };
    let (mut stream_read, stream_write) = stream.into_split();
    let (client_sender, client_receiver) = mpsc::channel(64);
    let (write_shutdown_send, mut write_shutdown_recv) = mpsc::channel(1);
    let client_id = Uuid::new_v4();
    spawn_and_log_error(
        client_write_loop(
            client_id,
            stream_write,
            client_receiver,
            write_shutdown_send,
        ),
        "client_write_loop",
    );
    let mut login_status = Connected {
        send: client_sender,
    };

    let mut received = Vec::with_capacity(1024);

    log::info!("Starting handler for new client with id {}", client_id);

    loop {
        tokio::select! {
            conn_alive = read_from_client(client_id, &mut stream_read, &mut received) =>
                if !conn_alive { break },
            _ = write_shutdown_recv.recv() => {
                log::info!("Writer for client {} shut down, stopping read handler", client_id);
                break
            },
        }
        login_status = match process_messages(
            client_id,
            &ip_addr,
            &mut received,
            &mut broker,
            login_status,
        )
        .await
        {
            Ok(status) => status,
            Err(e) => {
                log::error!("Error parsing message from client {}: {}", client_id, e);
                break;
            }
        };
    }
    log::info!("Client handler finished for client {}", client_id);
    broker.send(Event::DropClient { id: client_id }).await?;
    Ok(())
}

async fn process_messages(
    client_id: Uuid,
    ip_addr: &Ipv4Addr,
    received: &mut Vec<u8>,
    broker: &mut EventSender,
    mut login_status: LoginStatus,
) -> Result<LoginStatus> {
    while received.len() > 0 {
        let initially_available = received.len();
        login_status = match login_status {
            Connected { send } => process_ident(received, send).await?,
            Greeted { send, game_version } => {
                process_login(client_id, ip_addr, received, broker, send, game_version).await?
            }
            LoggedIn => process_commands(client_id, received, broker).await?,
        };
        if received.len() == initially_available {
            // no data was consumed, so need to wait for more data
            break;
        }
    }

    Ok(login_status)
}

async fn process_commands(
    client_id: Uuid,
    received: &mut Vec<u8>,
    broker: &mut EventSender,
) -> Result<LoginStatus> {
    match ClientCommand::try_parse(received)? {
        Some(msg) => {
            broker
                .send(Event::Command {
                    id: client_id,
                    command: msg,
                })
                .await?;
            Ok(LoggedIn)
        }
        None => Ok(LoggedIn),
    }
}

async fn process_login(
    client_id: Uuid,
    ip_addr: &Ipv4Addr,
    received: &mut Vec<u8>,
    broker: &mut EventSender,
    mut send: MessageSender,
    game_version: Uuid,
) -> Result<LoginStatus> {
    const ALLOWED_USERNAME_CHARS: &str =
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.|()[]{}";
    match LoginClientMessage::try_parse(received)? {
        Some(login) => {
            let username = bytevec_to_str(&login.username);
            if only_allowed_chars_not_empty(&username, ALLOWED_USERNAME_CHARS) {
                broker
                    .send(Event::NewUser {
                        id: client_id,
                        game_version,
                        send,
                        ip_addr: ip_addr.clone(),
                        username,
                    })
                    .await?;
                Ok(LoggedIn)
            } else {
                send.send(Arc::new(RejectServerMessage {
                    reason: "translateInvalidCharactersInName".to_string(),
                }))
                .await?;
                Ok(Greeted { send, game_version })
            }
        }
        None => Ok(Greeted { send, game_version }),
    }
}

async fn process_ident(received: &mut Vec<u8>, mut send: MessageSender) -> Result<LoginStatus> {
    let allowed_game_version: Uuid =
        Uuid::parse_str("534ba248-a87c-4ce9-8bee-bc376aae6134").unwrap();
    match IdentClientMessage::try_parse(received)? {
        Some(ident) => {
            if ident.game_version == allowed_game_version {
                send.send(Arc::new(IdentServerMessage {})).await?;
                Ok(Greeted {
                    send,
                    game_version: ident.game_version,
                })
            } else {
                send.send(Arc::new(RejectServerMessage {
                    reason: "Wrong game version. Please install version 2.2".to_string(),
                }))
                .await?;
                Ok(Connected { send })
            }
        }
        None => Ok(Connected { send }),
    }
}

async fn read_from_client(
    client_id: Uuid,
    reader: &mut (impl AsyncRead + Unpin),
    received: &mut Vec<u8>,
) -> bool {
    let mut read_buf = [0u8; 256];
    let num_read = match reader.read(&mut read_buf).await {
        Ok(0) => {
            log::info!("Client {} closed the connection", client_id);
            return false;
        }
        Ok(n) => n,
        Err(e) if e.kind() == ErrorKind::Interrupted || e.kind() == ErrorKind::WouldBlock => {
            return true
        }
        Err(e) => {
            log::warn!("Error when reading from client {}: {}", client_id, e);
            return false;
        }
    };
    received.extend_from_slice(&read_buf[..num_read]);
    true
}

async fn client_write_loop(
    client_id: Uuid,
    mut stream: OwnedWriteHalf,
    mut messages: MessageReceiver,
    _shutdown_send: mpsc::Sender<()>,
) -> Result<()> {
    while let Some(msg) = messages.next().await {
        log::debug!("Sending message to client {}: {:?}", client_id, msg);
        send_message(&*msg, &mut stream).await?;
    }
    log::info!("Writer for client {} is finished", client_id);
    Ok(())
}

async fn send_message(
    message: &dyn ServerMessage,
    writer: &mut (impl AsyncWrite + Unpin),
) -> Result<()> {
    let bytes = message.prepare_message()?;
    writer.write_all(&bytes).await?;
    Ok(())
}
