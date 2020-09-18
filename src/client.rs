use crate::broker::{Event, Receiver, Sender};
use crate::client::LoginStatus::LoggedIn;
use crate::messages::client_command::ClientCommand;
use crate::messages::login_client::{IdentClientMessage, LoginClientMessage};
use crate::messages::login_server::IdentServerMessage;
use crate::messages::ServerMessage;
use crate::server::spawn_and_log_error;
use anyhow::Result;
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
        send: Sender<Arc<dyn ServerMessage>>,
    },
    Greeted {
        send: Sender<Arc<dyn ServerMessage>>,
        game_version: Uuid,
    },
    LoggedIn,
}

pub async fn client_handler(stream: TcpStream, mut broker: Sender<Event>) -> Result<()> {
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
        login_status =
            match process_messages(client_id, &mut received, &mut broker, login_status).await {
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
    received: &mut Vec<u8>,
    broker: &mut Sender<Event>,
    mut login_status: LoginStatus,
) -> Result<LoginStatus> {
    while received.len() > 0 {
        login_status = match login_status {
            Connected { mut send } => match IdentClientMessage::try_parse(received)? {
                Some(ident) => {
                    send.send(Arc::new(IdentServerMessage {})).await?;
                    Greeted {
                        send,
                        game_version: ident.game_version,
                    }
                }
                None => return Ok(Connected { send }),
            },
            Greeted { send, game_version } => match LoginClientMessage::try_parse(received)? {
                Some(login) => {
                    broker
                        .send(Event::NewClient {
                            id: client_id,
                            send,
                            username: String::from_utf8(login.username)?,
                        })
                        .await?;
                    LoggedIn
                }
                None => return Ok(Greeted { send, game_version }),
            },
            LoggedIn => match ClientCommand::try_parse(received)? {
                Some(msg) => {
                    broker
                        .send(Event::Message {
                            id: client_id,
                            command: msg,
                        })
                        .await?;
                    LoggedIn
                }
                None => break,
            },
        }
    }

    Ok(login_status)
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
    mut messages: Receiver<Arc<dyn ServerMessage>>,
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
