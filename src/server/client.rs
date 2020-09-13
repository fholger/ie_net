use crate::server;
use crate::server::broker::{Event, Receiver, Sender};
use crate::server::messages::login_client::{
    IdentClientParams, LoginClientMessage, LoginClientParams,
};
use crate::server::messages::login_server::{IdentServerParams, LoginServerMessage};
use crate::server::messages::{ClientMessage, SendMessage, ServerMessage};
use anyhow::Result;
use async_std::io::BufReader;
use async_std::net::TcpStream;
use futures::channel::mpsc;
use futures::{AsyncReadExt, AsyncWrite, AsyncWriteExt, SinkExt, StreamExt};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ClientStatus {
    Connected,
    Greeted,
    LoggedIn,
}

pub async fn client_login_handler(stream: TcpStream, mut broker: Sender<Event>) -> Result<()> {
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
            Some(LoginClientMessage::Ident(params)) => {
                handle_ident(client_id, params, &mut write_stream).await?
            }
            Some(LoginClientMessage::Login(params)) => {
                handle_login(client_id, params, &mut write_stream).await?
            }
            None => continue,
        };
        log::debug!("New client status is {:?}", client_status);

        if client_status == ClientStatus::LoggedIn {
            log::info!("Login for client {} done, handing off...", client_id);
            let (writer_sender, writer_receiver) = mpsc::unbounded();
            broker
                .send(Event::NewClient {
                    uuid: client_id,
                    messages: writer_sender,
                })
                .await?;
            server::spawn_and_log_error(client_write_loop(stream.clone(), writer_receiver));
            server::spawn_and_log_error(client_read_loop(client_id, stream, broker));
            return Ok(());
        }
    }
    log::debug!("Client login handler finished for client {}", client_id);
    Ok(())
}

async fn client_read_loop(id: Uuid, stream: TcpStream, _broker: Sender<Event>) -> Result<()> {
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
        if let Some(_message) = ClientMessage::try_parse(&mut received)? {}
    }
    Ok(())
}

async fn client_write_loop(
    mut stream: TcpStream,
    mut messages: Receiver<ServerMessage>,
) -> Result<()> {
    while let Some(msg) = messages.next().await {
        log::debug!("Sending message to client {}: {:?}", "0", msg);
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

async fn handle_ident(
    client_id: Uuid,
    ident: IdentClientParams,
    writer: &mut (impl AsyncWrite + Unpin),
) -> Result<ClientStatus> {
    log::debug!("Received ident message from {}: {:?}", client_id, ident);
    // TODO: verify client game version
    let response = LoginServerMessage::Ident(IdentServerParams {});
    send_message(response, writer).await?;
    Ok(ClientStatus::Greeted)
}

async fn handle_login(
    client_id: Uuid,
    login: LoginClientParams,
    _writer: &mut (impl AsyncWrite + Unpin),
) -> Result<ClientStatus> {
    log::debug!("Received login message from {}: {:?}", client_id, login);
    // TODO: verify login
    Ok(ClientStatus::LoggedIn)
}
