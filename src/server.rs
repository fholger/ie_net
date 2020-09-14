use anyhow::Result;

use crate::broker::broker_loop;
use crate::client::client_handler;
use std::future::Future;
use tokio::net::TcpListener;
use tokio::stream::StreamExt;
use tokio::sync::mpsc;
use tokio::task;

pub async fn run(addr: &str) -> Result<()> {
    accept_loop(addr).await
}

async fn accept_loop(addr: &str) -> Result<()> {
    let mut listener = TcpListener::bind(addr).await?;
    log::info!("Listening for connections at {}", addr);

    let (broker_sender, broker_receiver) = mpsc::channel(256);
    let _main_handle = task::spawn(broker_loop(broker_receiver));

    let mut incoming_connections = listener.incoming();
    while let Some(connection) = incoming_connections.next().await {
        let connection = connection?;
        log::info!("New connection from {}", connection.peer_addr()?);
        spawn_and_log_error(client_handler(connection, broker_sender.clone()));
    }

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
