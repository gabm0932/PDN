//! Test client for podnext-daemon

use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let socket_path = format!("{}/.local/run/pn-next/daemon.sock", home);

    // Connect to daemon
    let mut stream = UnixStream::connect(socket_path).await?;
    println!("Connected to daemon");

    // Send ping
    let req = json!({"cmd": "ping"});
    let mut stream = stream;
    stream.write_all(serde_json::to_string(&req)?.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    // Read response
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;

    println!("Response: {}", line.trim());

    // Test list command
    let req = json!({"cmd": "list"});
    let mut stream = reader.into_inner();
    stream.write_all(serde_json::to_string(&req)?.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    line.clear();
    timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
    println!("List response: {}", line.trim());

    Ok(())
}