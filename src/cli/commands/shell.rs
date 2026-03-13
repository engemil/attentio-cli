use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::info;

use crate::device::connection::DeviceConnection;
use crate::device::discovery::resolve_device;

/// Execute the `shell` command — open an interactive ChibiOS shell session.
///
/// Reads lines from stdin, sends each as a command to the device,
/// and prints the response. Type `exit`, `quit`, or Ctrl+D to disconnect.
pub async fn execute(device: Option<&str>) -> Result<()> {
    // Resolve which device to talk to
    let dev = resolve_device(device).context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    eprintln!(
        "Connected to {} ({}). Type 'exit' or Ctrl+D to quit.",
        dev.serial, port_path
    );

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        eprint!("attentio> ");

        match lines.next_line().await? {
            None => {
                // EOF (Ctrl+D)
                eprintln!();
                break;
            }
            Some(line) => {
                let line = line.trim().to_string();

                if line.is_empty() {
                    continue;
                }

                if line == "exit" || line == "quit" {
                    break;
                }

                match conn.send_command(&line).await {
                    Ok(response) => {
                        if !response.is_empty() {
                            println!("{}", response);
                        }
                        println!("OK");
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                    }
                }
            }
        }
    }

    eprintln!("Disconnected.");
    Ok(())
}
