use anyhow::Result;

use crate::broker::{broker_loop, Event};
use crate::client::client_handler;
use std::future::Future;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task;

pub async fn run(addr: String) -> Result<()> {
    let (shutdown_send, shutdown_recv) = watch::channel(false);

    let (broker_sender, broker_receiver) = mpsc::channel(256);
    let broker_handle = spawn_and_log_error(
        broker_loop(broker_receiver, shutdown_recv.clone()),
        "broker_loop",
    );
    let accept_handle = spawn_and_log_error(
        accept_loop(addr, shutdown_recv.clone(), broker_sender),
        "accept_loop",
    );

    signal::ctrl_c().await?;
    log::info!("Received Ctrl-C event, shutting down");
    shutdown_send.broadcast(true)?;
    accept_handle.await?;
    broker_handle.await?;

    Ok(())
}

async fn accept_loop(
    addr: String,
    mut shutdown_recv: watch::Receiver<bool>,
    broker_sender: mpsc::Sender<Event>,
) -> Result<()> {
    let mut listener = TcpListener::bind(&addr).await?;
    log::info!("Listening for connections at {}", &addr);

    let mut incoming_connections = listener.incoming();
    loop {
        tokio::select! {
            Some(connection) = incoming_connections.next() => {
                let connection = connection?;
                log::info!("New connection established");
                spawn_and_log_error(client_handler(connection, broker_sender.clone()), "client_handler");
            },
            Some(shutdown) = shutdown_recv.recv() => if shutdown { break },
            else => break,
        }
    }

    log::info!("Accept loop shutting down");
    Ok(())
}

pub fn spawn_and_log_error<F>(future: F, description: &'static str) -> task::JoinHandle<()>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    task::spawn(async move {
        if let Err(e) = future.await {
            log::error!("Task {} exited with error: {}", description, e);
        }
    })
}
