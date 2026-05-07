//! Attentio Protocol (AP) interface.
//!
//! This module implements the client side of the AP protocol used to
//! communicate with AttentioLight-1 devices over CDC1 (the protocol port).

pub mod client;
pub mod crc;
pub mod packet;

// Re-export the main types for convenience.
pub use client::{open_client, open_client_for_device, ApClient, MonitorEvent};
