//! AP packet building, parsing, and protocol constants.
//!
//! Packet format: `[SYNC 0xA5][LEN][CMD][PAYLOAD 0..252][CRC8]`
//!
//! - **LEN** = length of CMD + PAYLOAD (1..253). Excludes SYNC, LEN, and CRC.
//! - **CRC** = CRC-8/CCITT over LEN + CMD + PAYLOAD bytes.
//! - Protocol is synchronous: one command, one response.

use super::crc::crc8;

// ── Framing ──────────────────────────────────────────────────────────────────

/// Sync byte that starts every AP packet.
pub const SYNC_BYTE: u8 = 0xA5;

/// Maximum payload length (252 bytes; LEN max = 253 = 1 cmd + 252 payload).
pub const MAX_PAYLOAD_LEN: usize = 252;

// ── Command IDs ──────────────────────────────────────────────────────────────

// Session control (0x00-0x0F)
pub const CMD_CLAIM: u8 = 0x01;
pub const CMD_RELEASE: u8 = 0x02;
pub const CMD_PING: u8 = 0x03;

// Power control (0x10-0x1F)
pub const CMD_POWER_ON: u8 = 0x10;
pub const CMD_POWER_OFF: u8 = 0x11;

// Direct LED control (0x20-0x2F)
pub const CMD_LED_OFF: u8 = 0x20;
pub const CMD_SET_RGB: u8 = 0x21;
pub const CMD_SET_HSV: u8 = 0x22;
pub const CMD_SET_BRIGHTNESS: u8 = 0x23;

// Query (0x40-0x4F)
pub const CMD_GET_STATUS: u8 = 0x40;
pub const CMD_GET_METADATA: u8 = 0x43;
pub const CMD_METADATA_GET: u8 = 0x44;

// Settings (0x50-0x5F)
pub const CMD_SETTINGS_LIST: u8 = 0x50;
pub const CMD_SETTINGS_GET: u8 = 0x51;
pub const CMD_SETTINGS_SET: u8 = 0x52;

// Log control (0x60-0x6F)
pub const CMD_LOG_GET_LEVEL: u8 = 0x60;
pub const CMD_LOG_SET_LEVEL: u8 = 0x61;

// DFU (0x70-0x7F)
pub const CMD_DFU_ENTER: u8 = 0x70;

// Events (0x80-0x8F) — Device -> Host (used by monitor)
pub const CMD_EVT_BUTTON: u8 = 0x80;
pub const CMD_EVT_STATE_CHANGE: u8 = 0x81;
pub const CMD_EVT_SESSION_END: u8 = 0x82;

/// Response OK — returned by the device on success.
pub const CMD_OK: u8 = 0xF0;
/// Response ERROR — returned by the device on failure.
pub const CMD_ERROR: u8 = 0xF1;

// ── AP error codes ───────────────────────────────────────────────────────────

pub const AP_ERR_NONE: u8 = 0x00;
pub const AP_ERR_NOT_CONTROLLER: u8 = 0x01;
pub const AP_ERR_INVALID_CMD: u8 = 0x02;
pub const AP_ERR_INVALID_PARAM: u8 = 0x03;
pub const AP_ERR_INVALID_STATE: u8 = 0x04;
pub const AP_ERR_CRC_FAIL: u8 = 0x05;

/// Human-readable description for an AP error code.
pub fn ap_error_name(code: u8) -> &'static str {
    match code {
        AP_ERR_NONE => "none",
        AP_ERR_NOT_CONTROLLER => "not active controller",
        AP_ERR_INVALID_CMD => "invalid/unknown command",
        AP_ERR_INVALID_PARAM => "invalid parameter",
        AP_ERR_INVALID_STATE => "invalid state",
        AP_ERR_CRC_FAIL => "CRC check failed",
        _ => "unknown error",
    }
}

// ── Command-id classification ────────────────────────────────────────────────

/// Inclusive range of host-to-device request commands (`AP_CMD_*`).
///
/// Covers the session/power/LED/query/settings/log/DFU sub-blocks
/// (`0x00–0x7F`) per the firmware's command-id layout in
/// `fw_al1mb1/app/attentio_protocol/attentio_protocol.h`.
pub const REQUEST_CMD_RANGE: std::ops::RangeInclusive<u8> = 0x00..=0x7F;

/// Inclusive range of device-to-host event commands (`AP_CMD_EVT_*`).
///
/// Mirrors the firmware's `0x80–0x8F` event block.
pub const EVENT_CMD_RANGE: std::ops::RangeInclusive<u8> = 0x80..=0x8F;

/// High-level classification of an AP command byte.
///
/// Derived from the firmware's documented command-id layout:
/// - `0x00–0x7F` → request (host → device)
/// - `0x80–0x8F` → event (device → host, unsolicited)
/// - `0xF0` ([`CMD_OK`]) and `0xF1` ([`CMD_ERROR`]) → response (device → host)
/// - everything else → reserved / unknown
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdClass {
    /// Host→device request.
    Request,
    /// Device→host response to a request (`CMD_OK` / `CMD_ERROR`).
    Response,
    /// Device→host unsolicited event (cmd in [`EVENT_CMD_RANGE`]).
    Event,
    /// Reserved / not assigned.
    Unknown,
}

impl CmdClass {
    /// Classify a raw command byte by the firmware's documented layout.
    pub const fn of(cmd: u8) -> Self {
        match cmd {
            CMD_OK | CMD_ERROR => Self::Response,
            0x80..=0x8F => Self::Event,
            0x00..=0x7F => Self::Request,
            _ => Self::Unknown,
        }
    }
}

// ── Packet building ──────────────────────────────────────────────────────────

/// Build a complete AP packet: `[SYNC][LEN][CMD][payload...][CRC8]`.
///
/// `payload` may be empty (e.g. GET_METADATA has no payload).
/// Panics if `payload.len() > MAX_PAYLOAD_LEN`.
pub fn build_packet(cmd: u8, payload: &[u8]) -> Vec<u8> {
    assert!(
        payload.len() <= MAX_PAYLOAD_LEN,
        "AP payload too large: {} > {}",
        payload.len(),
        MAX_PAYLOAD_LEN
    );

    let len = 1 + payload.len(); // CMD + payload
    let mut pkt = Vec::with_capacity(3 + payload.len() + 1); // SYNC + LEN + CMD + payload + CRC

    pkt.push(SYNC_BYTE);
    pkt.push(len as u8);
    pkt.push(cmd);
    pkt.extend_from_slice(payload);

    // CRC over LEN + CMD + PAYLOAD
    let crc = crc8(&pkt[1..]); // skip SYNC
    pkt.push(crc);

    pkt
}

// ── Response parsing ─────────────────────────────────────────────────────────

/// A parsed AP response from the device.
#[derive(Debug, Clone)]
pub struct ApResponse {
    /// Response command byte (CMD_OK or CMD_ERROR).
    pub cmd: u8,
    /// Response payload (may be empty).
    pub payload: Vec<u8>,
}

impl ApResponse {
    /// Returns `true` if this is a success response (CMD_OK).
    pub fn is_ok(&self) -> bool {
        self.cmd == CMD_OK
    }

    /// Returns `true` if this is an error response (CMD_ERROR).
    pub fn is_error(&self) -> bool {
        self.cmd == CMD_ERROR
    }

    /// If this is an error response, return the error code byte.
    pub fn error_code(&self) -> Option<u8> {
        if self.is_error() && !self.payload.is_empty() {
            Some(self.payload[0])
        } else {
            None
        }
    }

    /// Returns `true` if this is an unsolicited device-to-host event
    /// (cmd in [`EVENT_CMD_RANGE`]).
    pub fn is_event(&self) -> bool {
        matches!(CmdClass::of(self.cmd), CmdClass::Event)
    }
}

/// Parser state machine for receiving AP packets byte-by-byte.
///
/// Mirrors the firmware's `ap_parse_byte()` state machine:
///   SYNC -> LEN -> DATA (CMD + PAYLOAD) -> CRC
///
/// Non-sync bytes received in the SYNC state are silently discarded (resync).
#[derive(Debug)]
pub struct ApParser {
    state: ParseState,
    len: u8,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    Sync,
    Len,
    Data,
    Crc,
}

impl ApParser {
    /// Create a new parser in the initial SYNC state.
    pub fn new() -> Self {
        Self {
            state: ParseState::Sync,
            len: 0,
            data: Vec::new(),
        }
    }

    /// Reset the parser to the initial SYNC state.
    pub fn reset(&mut self) {
        self.state = ParseState::Sync;
        self.len = 0;
        self.data.clear();
    }

    /// Feed a single byte into the parser.
    ///
    /// Returns `Some(ApResponse)` when a complete, valid packet has been received.
    /// Returns `None` if more bytes are needed or if a CRC error causes a reset.
    pub fn feed(&mut self, byte: u8) -> Option<ApResponse> {
        match self.state {
            ParseState::Sync => {
                if byte == SYNC_BYTE {
                    self.state = ParseState::Len;
                }
                // Non-sync bytes are silently discarded (resync behavior).
                None
            }
            ParseState::Len => {
                if byte == 0 || byte as usize > MAX_PAYLOAD_LEN + 1 {
                    // Invalid length — reset and look for next sync.
                    self.reset();
                    None
                } else {
                    self.len = byte;
                    self.data.clear();
                    self.data.reserve(byte as usize);
                    self.state = ParseState::Data;
                    None
                }
            }
            ParseState::Data => {
                self.data.push(byte);
                if self.data.len() >= self.len as usize {
                    self.state = ParseState::Crc;
                }
                None
            }
            ParseState::Crc => {
                // Verify CRC over LEN + DATA (CMD + PAYLOAD)
                let mut crc_data = Vec::with_capacity(1 + self.data.len());
                crc_data.push(self.len);
                crc_data.extend_from_slice(&self.data);
                let expected_crc = crc8(&crc_data);

                let result = if byte == expected_crc {
                    let cmd = self.data[0];
                    let payload = if self.data.len() > 1 {
                        self.data[1..].to_vec()
                    } else {
                        Vec::new()
                    };
                    Some(ApResponse { cmd, payload })
                } else {
                    // CRC mismatch — discard packet
                    None
                };

                self.reset();
                result
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_packet_no_payload() {
        // DFU_ENTER: no payload
        let pkt = build_packet(CMD_DFU_ENTER, &[]);
        assert_eq!(pkt, vec![0xA5, 0x01, 0x70, 0x42]);
    }

    #[test]
    fn test_build_packet_with_payload() {
        // GET_METADATA: no payload
        let pkt = build_packet(CMD_GET_METADATA, &[]);
        assert_eq!(pkt.len(), 4); // SYNC + LEN + CMD + CRC
        assert_eq!(pkt[0], SYNC_BYTE);
        assert_eq!(pkt[1], 0x01); // LEN = 1 (just CMD)
        assert_eq!(pkt[2], CMD_GET_METADATA);
        // CRC over [0x01, 0x43]
        assert_eq!(pkt[3], crc8(&[0x01, 0x43]));
    }

    #[test]
    fn test_parser_ok_response_no_payload() {
        // Bare OK: [0xA5, 0x01, 0xF0, CRC]
        let pkt = build_packet(CMD_OK, &[]);
        let mut parser = ApParser::new();
        let mut result = None;
        for &b in &pkt {
            if let Some(r) = parser.feed(b) {
                result = Some(r);
            }
        }
        let resp = result.expect("should parse OK response");
        assert!(resp.is_ok());
        assert!(resp.payload.is_empty());
    }

    #[test]
    fn test_parser_error_response() {
        // ERROR with code 0x02: [0xA5, 0x02, 0xF1, 0x02, CRC]
        let pkt = build_packet(CMD_ERROR, &[AP_ERR_INVALID_CMD]);
        let mut parser = ApParser::new();
        let mut result = None;
        for &b in &pkt {
            if let Some(r) = parser.feed(b) {
                result = Some(r);
            }
        }
        let resp = result.expect("should parse ERROR response");
        assert!(resp.is_error());
        assert_eq!(resp.error_code(), Some(AP_ERR_INVALID_CMD));
    }

    #[test]
    fn test_parser_with_garbage_prefix() {
        // Garbage bytes before a valid packet should be skipped
        let pkt = build_packet(CMD_OK, &[]);
        let mut data = vec![0x00, 0xFF, 0x12]; // garbage
        data.extend_from_slice(&pkt);

        let mut parser = ApParser::new();
        let mut result = None;
        for &b in &data {
            if let Some(r) = parser.feed(b) {
                result = Some(r);
            }
        }
        let resp = result.expect("should parse OK after garbage");
        assert!(resp.is_ok());
    }

    #[test]
    fn test_parser_crc_mismatch() {
        // Corrupt CRC
        let mut pkt = build_packet(CMD_OK, &[]);
        *pkt.last_mut().unwrap() ^= 0xFF; // flip CRC bits

        let mut parser = ApParser::new();
        let mut result = None;
        for &b in &pkt {
            if let Some(r) = parser.feed(b) {
                result = Some(r);
            }
        }
        assert!(
            result.is_none(),
            "CRC mismatch should not produce a response"
        );
    }

    #[test]
    fn test_cmd_class_known_commands() {
        use CmdClass::*;
        // Requests
        assert_eq!(CmdClass::of(CMD_CLAIM), Request);
        assert_eq!(CmdClass::of(CMD_RELEASE), Request);
        assert_eq!(CmdClass::of(CMD_PING), Request);
        assert_eq!(CmdClass::of(CMD_POWER_ON), Request);
        assert_eq!(CmdClass::of(CMD_SET_RGB), Request);
        assert_eq!(CmdClass::of(CMD_GET_STATUS), Request);
        assert_eq!(CmdClass::of(CMD_SETTINGS_LIST), Request);
        assert_eq!(CmdClass::of(CMD_LOG_GET_LEVEL), Request);
        assert_eq!(CmdClass::of(CMD_DFU_ENTER), Request);
        // Events
        assert_eq!(CmdClass::of(CMD_EVT_BUTTON), Event);
        assert_eq!(CmdClass::of(CMD_EVT_STATE_CHANGE), Event);
        assert_eq!(CmdClass::of(CMD_EVT_SESSION_END), Event);
        // Responses
        assert_eq!(CmdClass::of(CMD_OK), Response);
        assert_eq!(CmdClass::of(CMD_ERROR), Response);
    }

    #[test]
    fn test_cmd_class_boundaries() {
        use CmdClass::*;
        assert_eq!(CmdClass::of(0x00), Request);
        assert_eq!(CmdClass::of(0x7F), Request);
        assert_eq!(CmdClass::of(0x80), Event);
        assert_eq!(CmdClass::of(0x8F), Event);
        assert_eq!(CmdClass::of(0x90), Unknown);
        assert_eq!(CmdClass::of(0xEF), Unknown);
        assert_eq!(CmdClass::of(0xF0), Response); // CMD_OK
        assert_eq!(CmdClass::of(0xF1), Response); // CMD_ERROR
        assert_eq!(CmdClass::of(0xF2), Unknown);
        assert_eq!(CmdClass::of(0xFF), Unknown);
    }

    #[test]
    fn test_apresponse_is_event() {
        let evt = ApResponse {
            cmd: CMD_EVT_BUTTON,
            payload: vec![0x01],
        };
        assert!(evt.is_event());
        assert!(!evt.is_ok());
        assert!(!evt.is_error());

        let ok = ApResponse {
            cmd: CMD_OK,
            payload: vec![],
        };
        assert!(!ok.is_event());

        let err = ApResponse {
            cmd: CMD_ERROR,
            payload: vec![AP_ERR_INVALID_CMD],
        };
        assert!(!err.is_event());
    }

    #[test]
    fn test_event_cmd_range_matches_constants() {
        assert!(EVENT_CMD_RANGE.contains(&CMD_EVT_BUTTON));
        assert!(EVENT_CMD_RANGE.contains(&CMD_EVT_STATE_CHANGE));
        assert!(EVENT_CMD_RANGE.contains(&CMD_EVT_SESSION_END));
        assert_eq!(*EVENT_CMD_RANGE.start(), 0x80);
        assert_eq!(*EVENT_CMD_RANGE.end(), 0x8F);
    }

    #[test]
    fn test_request_cmd_range_bounds() {
        assert_eq!(*REQUEST_CMD_RANGE.start(), 0x00);
        assert_eq!(*REQUEST_CMD_RANGE.end(), 0x7F);
        assert!(REQUEST_CMD_RANGE.contains(&CMD_CLAIM));
        assert!(REQUEST_CMD_RANGE.contains(&CMD_DFU_ENTER));
    }
}
