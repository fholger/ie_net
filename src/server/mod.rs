mod messages;

use anyhow::Result;
use async_std::future::Future;
use async_std::io::BufReader;
use async_std::net::TcpStream;
use async_std::{net::TcpListener, task};
use futures::{AsyncReadExt, StreamExt};
use std::collections::VecDeque;
use uuid::Uuid;
use messages::login::LoginClientMessage;

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

    log::info!("Starting login for new client with id {}", client_id);

    loop {
        let num_read = reader.read(&mut read_buf).await?;
        if num_read == 0 {
            log::info!("Client {} closed the connection", client_id);
            break;
        }
        received.extend_from_slice(&read_buf[..num_read]);

        match LoginClientMessage::try_parse(&mut received, client_status)? {
            Some(LoginClientMessage::Ident(params)) => continue,
            Some(LoginClientMessage::Login(params)) => continue,
            None => continue,
        }
    }
    log::debug!("Client loop finished for client {}", client_id);
    Ok(())
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
