mod broker;
mod client;
mod messages;

use anyhow::Result;
use async_std::future::Future;

use async_std::{net::TcpListener, task};
use broker::broker_loop;
use client::client_handler;
use futures::channel::mpsc;
use futures::StreamExt;

pub fn run(addr: &str) -> Result<()> {
    let accept_task = accept_loop(addr);
    task::block_on(accept_task)
}

async fn accept_loop(addr: &str) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    log::info!("Listening for connections at {}", addr);

    let (main_sender, main_receiver) = mpsc::unbounded();
    let _main_handle = task::spawn(broker_loop(main_receiver));

    let mut incoming_connections = listener.incoming();
    while let Some(connection) = incoming_connections.next().await {
        let connection = connection?;
        log::info!("New connection from {}", connection.peer_addr()?);
        spawn_and_log_error(client_handler(connection, main_sender.clone()));
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
