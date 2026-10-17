//! Copies a local file to a RouterOS device over SCP, to check `device::terminal::upload`
//! against a test device (e.g. the CHR sandbox).
//!
//!     cargo run -p backend --example upload_check -- <address:port> <user> <password> <file>
use backend::device::terminal::upload;
use std::path::Path;
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [address, user, password, file] = args.as_slice() else {
        anyhow::bail!("usage: <address:port> <user> <password> <file>");
    };
    let data = tokio::fs::read(file).await?;
    let name = Path::new(file).file_name().unwrap().to_string_lossy();
    let stream = TcpStream::connect(address.as_str()).await?;
    upload(stream, user, Some(password), &name, &data, &|sent| println!("{sent} bytes")).await?;
    println!("uploaded {name}, {} bytes", data.len());
    Ok(())
}
