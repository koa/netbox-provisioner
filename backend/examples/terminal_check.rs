//! Logs in to a RouterOS console over SSH and runs commands, to check `device::terminal`
//! against a test device (e.g. the CHR sandbox).
//!
//!     cargo run -p backend --example terminal_check -- <address:port> <user> <password> <command>...
use backend::device::terminal::Terminal;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().filter_or("LOG_LEVEL", "warn"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [address, user, password, commands @ ..] = args.as_slice() else {
        anyhow::bail!("usage: <address:port> <user> <password> <command>...");
    };
    let mut terminal = Terminal::connect(address.parse()?, user, Some(password)).await?;
    for command in commands {
        let output = terminal.run(command, Duration::from_secs(20)).await?;
        println!("> {command}\n{output}");
    }
    terminal.close().await;
    Ok(())
}
