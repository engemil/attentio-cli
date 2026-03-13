use anyhow::{Context, Result};
use serde_json::json;
use tracing::info;

use crate::device::connection::DeviceConnection;
use crate::device::discovery::resolve_device;
use crate::json_output;

/// Execute the `send` command — send a one-shot command to the device and print the response.
pub async fn execute(cmd: &[String], device: Option<&str>, json: bool) -> Result<()> {
    // Check if command is empty
    if cmd.is_empty() {
        if json {
            let error = json!({
                "status": "ERROR",
                "error": "No command provided"
            });
            println!("{}", serde_json::to_string_pretty(&error).unwrap());
            std::process::exit(1);
        } else {
            eprintln!("Error: No command provided\n");
            eprintln!("Usage: attentio send <cmd> [args...] [--device <serial>]\n");
            eprintln!("Example:");
            eprintln!("  attentio send help");
            std::process::exit(1);
        }
    }

    // Join multiple arguments into a single command string with smart quoting
    let cmd_str = quote_args_for_shell(cmd);

    // Execute the command and handle both success and error cases
    match execute_internal(&cmd_str, device).await {
        Ok((device_serial, response)) => {
            if json {
                let output = json!({
                    "device": device_serial,
                    "command": cmd_str,
                    "response": response,
                });
                println!("{}", json_output::format_success(output));
            } else {
                if !response.is_empty() {
                    println!("{}", response);
                }
                println!("OK");
            }
            Ok(())
        }
        Err(e) => {
            if json {
                let context = json!({
                    "command": cmd_str,
                });
                println!("{}", json_output::format_error(&e, context));
            } else {
                eprintln!("Error: {:#}", e);
            }
            Err(e)
        }
    }
}

/// Internal implementation that returns result data.
async fn execute_internal(cmd_str: &str, device: Option<&str>) -> Result<(String, String)> {
    // Resolve which device to talk to
    let dev = resolve_device(device).context("failed to resolve device")?;
    let port_path = dev
        .shell_port()
        .ok_or_else(|| anyhow::anyhow!("device '{}' has no shell port", dev.serial))?;

    info!("Connecting to {} on {}", dev.serial, port_path);

    // Open connection and send the command
    let mut conn = DeviceConnection::open(port_path)
        .context(format!("failed to open serial port {}", port_path))?;

    let response = conn
        .send_command(cmd_str)
        .await
        .context(format!("failed to send command '{}'", cmd_str))?;

    Ok((dev.serial.clone(), response))
}

/// Quote arguments for the ChibiOS shell.
///
/// Automatically wraps arguments containing whitespace in double quotes,
/// and escapes any existing double quotes within those arguments.
/// This preserves the user's intent: quoted strings in bash → quoted strings on device.
///
/// Examples:
/// - `["echo", "test"]` → `"echo test"`
/// - `["echo", "test this"]` → `"echo \"test this\""`
/// - `["led", "pulse", "red"]` → `"led pulse red"`
fn quote_args_for_shell(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.chars().any(char::is_whitespace) {
                // Escape existing double quotes and wrap in quotes
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_args_single_word() {
        let args = vec!["echo".to_string(), "test".to_string()];
        assert_eq!(quote_args_for_shell(&args), "echo test");
    }

    #[test]
    fn test_quote_args_with_spaces() {
        let args = vec!["echo".to_string(), "test this".to_string()];
        assert_eq!(quote_args_for_shell(&args), "echo \"test this\"");
    }

    #[test]
    fn test_quote_args_multiple_with_spaces() {
        let args = vec![
            "cmd".to_string(),
            "arg one".to_string(),
            "arg two".to_string(),
        ];
        assert_eq!(quote_args_for_shell(&args), "cmd \"arg one\" \"arg two\"");
    }

    #[test]
    fn test_quote_args_mixed() {
        let args = vec![
            "led".to_string(),
            "pulse".to_string(),
            "bright red".to_string(),
        ];
        assert_eq!(quote_args_for_shell(&args), "led pulse \"bright red\"");
    }

    #[test]
    fn test_quote_args_with_tabs() {
        let args = vec!["echo".to_string(), "test\tthis".to_string()];
        assert_eq!(quote_args_for_shell(&args), "echo \"test\tthis\"");
    }

    #[test]
    fn test_quote_args_with_embedded_quotes() {
        let args = vec!["echo".to_string(), "He said \"hello\"".to_string()];
        assert_eq!(
            quote_args_for_shell(&args),
            "echo \"He said \\\"hello\\\"\""
        );
    }

    #[test]
    fn test_quote_args_empty() {
        let args: Vec<String> = vec![];
        assert_eq!(quote_args_for_shell(&args), "");
    }

    #[test]
    fn test_quote_args_single_command() {
        let args = vec!["help".to_string()];
        assert_eq!(quote_args_for_shell(&args), "help");
    }
}
