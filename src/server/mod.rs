use async_std::{task, net::TcpListener};
use anyhow::Result;
use futures::StreamExt;

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
    }

    Ok(())
}
