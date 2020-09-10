mod messages;

use anyhow::Result;
use async_std::future::Future;
use async_std::io::BufReader;
use async_std::net::TcpStream;
use async_std::{net::TcpListener, task};
use futures::{AsyncReadExt, StreamExt};
use std::collections::VecDeque;
use uuid::Uuid;

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
        spawn_and_log_error(client_loop(connection));
    }

    Ok(())
}

async fn client_loop(stream: TcpStream) -> Result<()> {
    let client_addr = stream.peer_addr()?;
    let client_id = Uuid::new_v4();
    let mut reader = BufReader::new(&stream);
    let mut read_buf = [0u8; 1024];
    let mut received: VecDeque<u8> = VecDeque::with_capacity(1024);

    log::info!(
        "Begin reading from client {} with generated client id {}",
        client_addr,
        client_id
    );

    loop {
        let num_read = reader.read(&mut read_buf).await?;
        if num_read == 0 {
            break;
        }
        received.extend(&read_buf[..num_read]);
    }
    log::info!("Client loop finished for {}", client_addr);
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
