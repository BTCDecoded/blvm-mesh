//! Edge link kinds for radio / Meshtastic / Reticulum-style bridges.
//!
//! Used by `blvm-bridge` and `MeshClient` payload chunking for LoRa-side limits.

use std::fmt;

/// Conservative byte budget for **one application payload** on a Meshtastic-style LoRa frame
/// after headers / bookkeeping (planning figure for chunking only).
pub const MESHTASTIC_LORA_APP_PAYLOAD_PLANNING_MAX: usize = 200;

/// Parsed `BRIDGE_EDGE_TRANSPORT` / connection hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeTransportKind {
    Meshtastic,
    MeshtasticMqtt,
    Reticulum,
    GenericRadio,
}

impl fmt::Display for EdgeTransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Meshtastic => "meshtastic",
            Self::MeshtasticMqtt => "meshtastic-mqtt",
            Self::Reticulum => "reticulum",
            Self::GenericRadio => "generic-radio",
        };
        f.write_str(s)
    }
}

/// Parse a transport label (from env or config). Returns `None` if unrecognized.
pub fn parse_edge_transport(raw: &str) -> Option<EdgeTransportKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "meshtastic" => Some(EdgeTransportKind::Meshtastic),
        "meshtastic-mqtt" | "meshtastic_mqtt" => Some(EdgeTransportKind::MeshtasticMqtt),
        "reticulum" => Some(EdgeTransportKind::Reticulum),
        "generic-radio" | "generic_radio" | "radio" => Some(EdgeTransportKind::GenericRadio),
        _ => None,
    }
}

/// Split `data` into non-empty chunks of at most `chunk_size` bytes (`chunk_size` clamped ≥ 1).
pub fn chunk_bytes(data: &[u8], chunk_size: usize) -> impl Iterator<Item = &[u8]> + '_ {
    let sz = chunk_size.max(1);
    data.chunks(sz)
}
