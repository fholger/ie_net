use anyhow::Result;

use crate::broker::{broker_loop, Event};
use crate::client::client_handler;
use std::future::Future;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task;
use tokio::task::JoinHandle;

pub async fn run(addr: String) -> Result<()> {
    let (shutdown_send, shutdown_recv) = watch::channel(false);

    let (broker_sender, broker_receiver) = mpsc::channel(256);
    let mut broker_handle = spawn_and_log_error(
        broker_loop(broker_receiver, shutdown_recv.clone()),
        "broker_loop",
    );
    let mut accept_handle = spawn_and_log_error(
        accept_loop(addr, shutdown_recv.clone(), broker_sender),
        "accept_loop",
    );

    let result = shutdown_watch(&mut accept_handle, &mut broker_handle).await;
    log::info!("Shutting down server");
    shutdown_send.broadcast(true)?;
    accept_handle.await?;
    broker_handle.await?;

    result
}

async fn shutdown_watch(
    accept_handle: &mut JoinHandle<()>,
    broker_handle: &mut JoinHandle<()>,
) -> Result<()> {
    tokio::select! {
        result = accept_handle => result?,
        result = broker_handle => result?,
        result = signal_watch() => {
            log::info!("Received shutdown signal");
            result?
        }
    };
    Ok(())
}

#[cfg(target_family = "windows")]
async fn signal_watch() -> Result<()> {
    Ok(signal::ctrl_c().await?)
}

#[cfg(target_family = "unix")]
async fn signal_watch() -> Result<()> {
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = signal::ctrl_c() => result?,
        _ = sighup.recv() => (),
        _ = sigterm.recv() => (),
    }
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
