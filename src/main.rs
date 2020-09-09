use anyhow::Result;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Options {
    #[structopt(short, long, default_value="17171")]
    /// Port to listen for connections from the game clients
    port: u16,
}

fn main() -> Result<()> {
    let options = Options::from_args();

    flexi_logger::Logger::with_env_or_str("debug").start()?;
    log::info!("{:?}", options);

    Ok(())
}
