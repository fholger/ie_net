#[macro_use]
extern crate nom;

mod broker;
mod client;
mod messages;
mod server;
mod util;

use anyhow::Result;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Options {
    #[structopt(short, long, default_value = "0.0.0.0:17171")]
    /// Listening address/port to receive connections from game clients
    bind: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let options = Options::from_args();

    flexi_logger::Logger::with_env_or_str("debug").start()?;
    log::info!("IE::Net server starting up...");

    server::run(options.bind).await
}
