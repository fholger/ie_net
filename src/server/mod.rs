mod messages;

use anyhow::Result;
use async_std::future::Future;
use async_std::io::{BufReader, Write};
use async_std::net::TcpStream;
use async_std::{net::TcpListener, task};
use futures::{AsyncReadExt, StreamExt, AsyncWriteExt};
use uuid::Uuid;
use messages::login::LoginClientMessage;
use crate::server::messages::login::{IdentClientParams, LoginClientParams, LoginServerMessage, IdentServerParams, RejectServerParams, WelcomeServerParams};
use crate::server::messages::SendMessage;

pub fn run(addr: &str) -> Result<()> {
    let accept_task = accept_loop(addr);
    task::block_on(accept_task)
}

async fn accept_loop(addr: &str) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    log::info!("Listening for connections at {}", addr);

    let mut incoming_connections = listener.incoming();
    while let Some(connection) = incoming_connections.next().await {
        let connection = connection?;
        log::info!("New connection from {}", connection.peer_addr()?);
        spawn_and_log_error(client_login_handler(connection));
    }

    Ok(())
}

#[derive(Clone, Copy)]
pub enum ClientStatus {
    Connected,
    Greeted,
    LoggedIn,
}

async fn client_login_handler(stream: TcpStream) -> Result<()> {
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
        }
    }
    log::debug!("Client loop finished for client {}", client_id);
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

async fn handle_login(client_id: Uuid, login: LoginClientParams, writer: &mut (impl Write + Unpin)) -> Result<ClientStatus> {
    log::debug!("Received login message from {}: {:?}", client_id, login);
    //let response = LoginServerMessage::Reject(RejectServerParams { reason: "Nope. Go away.".to_string() });
    let response = LoginServerMessage::Welcome(WelcomeServerParams {
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
    });
    send_message(response, writer).await?;
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
