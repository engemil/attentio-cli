//! Human-readable formatting for AP protocol packets displayed in the monitor.

use crate::protocol::packet::*;

/// Format an outgoing AP command (host → device) for display.
pub fn format_outgoing(cmd: u8, payload: &[u8]) -> String {
    let name = cmd_name(cmd);
    let detail = format_cmd_payload(cmd, payload);
    if detail.is_empty() {
        format!("→ {}", name)
    } else {
        format!("→ {} {}", name, detail)
    }
}

/// Format an incoming AP response/event (device → host) for display.
pub fn format_incoming(resp: &ApResponse) -> String {
    match resp.cmd {
        CMD_OK => {
            if resp.payload.is_empty() {
                "← OK".to_string()
            } else {
                format!("← OK [{}]", format_hex(&resp.payload))
            }
        }
        CMD_ERROR => {
            let err = resp.payload.first().copied().unwrap_or(0);
            format!("← ERROR: {}", ap_error_name(err))
        }
        CMD_EVT_BUTTON => {
            let event = resp.payload.first().copied().unwrap_or(0);
            format!("← EVT_BUTTON {}", button_event_name(event))
        }
        CMD_EVT_STATE_CHANGE => {
            format!("← EVT_STATE_CHANGE [{}]", format_hex(&resp.payload))
        }
        CMD_EVT_SESSION_END => {
            let reason = resp.payload.first().copied().unwrap_or(0);
            let session_id = if resp.payload.len() >= 3 {
                u16::from_be_bytes([resp.payload[1], resp.payload[2]])
            } else {
                0
            };
            if session_id > 0 {
                format!(
                    "← EVT_SESSION_END {} (session {})",
                    session_end_reason(reason),
                    session_id
                )
            } else {
                format!("← EVT_SESSION_END {}", session_end_reason(reason))
            }
        }
        _ => {
            // Unknown incoming packet — show raw
            if resp.payload.is_empty() {
                format!("← CMD:0x{:02X}", resp.cmd)
            } else {
                format!("← CMD:0x{:02X} [{}]", resp.cmd, format_hex(&resp.payload))
            }
        }
    }
}

/// Human-readable name for a command ID.
fn cmd_name(cmd: u8) -> &'static str {
    match cmd {
        CMD_CLAIM => "CLAIM",
        CMD_RELEASE => "RELEASE",
        CMD_PING => "PING",
        CMD_POWER_ON => "POWER_ON",
        CMD_POWER_OFF => "POWER_OFF",
        CMD_LED_OFF => "LED_OFF",
        CMD_SET_RGB => "SET_RGB",
        CMD_SET_HSV => "SET_HSV",
        CMD_SET_BRIGHTNESS => "SET_BRIGHTNESS",
        CMD_GET_STATUS => "GET_STATUS",
        CMD_GET_METADATA => "GET_METADATA",
        CMD_METADATA_GET => "METADATA_GET",
        CMD_SETTINGS_LIST => "SETTINGS_LIST",
        CMD_SETTINGS_GET => "SETTINGS_GET",
        CMD_SETTINGS_SET => "SETTINGS_SET",
        CMD_LOG_GET_LEVEL => "LOG_GET_LEVEL",
        CMD_LOG_SET_LEVEL => "LOG_SET_LEVEL",
        CMD_DFU_ENTER => "DFU_ENTER",
        CMD_OK => "OK",
        CMD_ERROR => "ERROR",
        CMD_EVT_BUTTON => "EVT_BUTTON",
        CMD_EVT_STATE_CHANGE => "EVT_STATE_CHANGE",
        CMD_EVT_SESSION_END => "EVT_SESSION_END",
        _ => "UNKNOWN",
    }
}

/// Format command payload for display (outgoing direction).
fn format_cmd_payload(cmd: u8, payload: &[u8]) -> String {
    match cmd {
        CMD_SET_RGB if payload.len() >= 3 => {
            format!("[R:{} G:{} B:{}]", payload[0], payload[1], payload[2])
        }
        CMD_SET_HSV if payload.len() >= 4 => {
            let h = u16::from_le_bytes([payload[0], payload[1]]);
            format!("[H:{} S:{} V:{}]", h, payload[2], payload[3])
        }
        CMD_SET_BRIGHTNESS if !payload.is_empty() => {
            format!("[{}%]", payload[0])
        }
        CMD_LOG_SET_LEVEL if !payload.is_empty() => {
            let name = match payload[0] {
                0 => "NONE",
                1 => "ERROR",
                2 => "WARN",
                3 => "INFO",
                4 => "DEBUG",
                _ => "?",
            };
            format!("[{} ({})]", payload[0], name)
        }
        _ if !payload.is_empty() => format!("[{}]", format_hex(payload)),
        _ => String::new(),
    }
}

fn button_event_name(code: u8) -> &'static str {
    match code {
        0x01 => "SHORT_PRESS",
        0x02 => "LONG_PRESS_START",
        0x03 => "LONG_PRESS_RELEASE",
        0x04 => "EXTENDED_PRESS_START",
        0x05 => "EXTENDED_PRESS_RELEASE",
        _ => "UNKNOWN",
    }
}

fn session_end_reason(code: u8) -> &'static str {
    match code {
        0x00 => "RELEASED",
        0x01 => "TAKEOVER",
        0x02 => "POWEROFF",
        _ => "UNKNOWN",
    }
}

fn format_hex(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}
