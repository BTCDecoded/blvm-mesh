//! Routing policy determination for mesh networking
//!
//! This module determines whether a message requires payment based on protocol detection.
//! It leverages existing Bitcoin protocol detection rather than creating duplicate logic.

use crate::error::MeshError;
use std::sync::Arc;
use tracing::{debug, trace};

/// Routing policy for mesh messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingPolicy {
    /// Free routing (Bitcoin P2P, Commons governance, Stratum V2)
    Free,
    /// Payment required (arbitrary data, messaging, IPFS)
    PaymentRequired,
}

/// Detected protocol from message analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedProtocol {
    /// Bitcoin P2P protocol (detected via magic bytes + command)
    BitcoinP2P,
    /// Commons governance messages (detected via service flags + message type)
    CommonsGovernance,
    /// Stratum V2 mining protocol
    StratumV2,
    /// Mesh packet (detected via mesh magic)
    MeshPacket,
    /// Unknown protocol (requires payment)
    Unknown,
}

/// Mesh operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshMode {
    /// Bitcoin-only mode (only Bitcoin P2P allowed, no mesh)
    BitcoinOnly,
    /// Payment-gated mode (free protocols + paid mesh)
    PaymentGated,
    /// Open mode (all traffic free)
    Open,
}

/// Routing policy engine
pub struct RoutingPolicyEngine {
    mode: MeshMode,
}

impl RoutingPolicyEngine {
    /// Create a new routing policy engine
    pub fn new(mode: MeshMode) -> Self {
        Self { mode }
    }

    /// Detect protocol from message bytes
    /// 
    /// This leverages existing Bitcoin protocol detection patterns:
    /// - Bitcoin P2P: Magic bytes (0xf9beb4d9 for mainnet) + command string
    /// - Commons Governance: Service flags + specific message types
    /// - Stratum V2: Protocol-specific headers
    /// - Mesh: Mesh packet magic
    pub fn detect_protocol(&self, message: &[u8]) -> DetectedProtocol {
        // Fast-path: Check for Bitcoin P2P magic bytes first
        if message.len() >= 4 {
            let magic = u32::from_le_bytes([message[0], message[1], message[2], message[3]]);
            
            // Bitcoin P2P magic bytes
            if magic == 0xd9b4bef9 // mainnet
                || magic == 0x0709110b // testnet
                || magic == 0xdab5bffa // regtest
            {
                // Check if it's a known Bitcoin P2P command
                if message.len() >= 12 {
                    let command = String::from_utf8_lossy(&message[4..12])
                        .trim_end_matches('\0')
                        .to_string();
                    
                    // Known Bitcoin P2P commands
                    if self.is_bitcoin_command(&command) {
                        trace!("Detected Bitcoin P2P protocol: command={}", command);
                        return DetectedProtocol::BitcoinP2P;
                    }
                    
                    // Check for Commons governance messages
                    if self.is_governance_command(&command) {
                        trace!("Detected Commons governance protocol: command={}", command);
                        return DetectedProtocol::CommonsGovernance;
                    }
                }
            }
        }
        
        // Check for Stratum V2 protocol (if message is long enough)
        if message.len() >= 2 {
            // Stratum V2 uses specific message type tags
            // This is a simplified check - full detection would parse the protocol
            if self.is_stratum_v2_message(message) {
                trace!("Detected Stratum V2 protocol");
                return DetectedProtocol::StratumV2;
            }
        }
        
        // Check for mesh packet magic
        if message.len() >= 4 && message[0] == b'M' && message[1] == b'E' 
            && message[2] == b'S' && message[3] == b'H' {
            trace!("Detected mesh packet");
            return DetectedProtocol::MeshPacket;
        }
        
        // Unknown protocol
        trace!("Unknown protocol detected");
        DetectedProtocol::Unknown
    }

    /// Determine routing policy based on protocol detection and mode
    pub fn determine_policy(&self, protocol: DetectedProtocol) -> RoutingPolicy {
        match (protocol, self.mode) {
            // Bitcoin P2P is always free
            (DetectedProtocol::BitcoinP2P, _) => {
                trace!("Bitcoin P2P → Free routing");
                RoutingPolicy::Free
            }
            
            // Commons governance is always free
            (DetectedProtocol::CommonsGovernance, _) => {
                trace!("Commons governance → Free routing");
                RoutingPolicy::Free
            }
            
            // Stratum V2 is always free
            (DetectedProtocol::StratumV2, _) => {
                trace!("Stratum V2 → Free routing");
                RoutingPolicy::Free
            }
            
            // Mesh packets depend on mode
            (DetectedProtocol::MeshPacket, MeshMode::BitcoinOnly) => {
                debug!("Mesh packet in Bitcoin-only mode → Rejected");
                RoutingPolicy::PaymentRequired // Effectively rejected
            }
            (DetectedProtocol::MeshPacket, MeshMode::PaymentGated) => {
                trace!("Mesh packet in payment-gated mode → Payment required");
                RoutingPolicy::PaymentRequired
            }
            (DetectedProtocol::MeshPacket, MeshMode::Open) => {
                trace!("Mesh packet in open mode → Free routing");
                RoutingPolicy::Free
            }
            
            // Unknown protocols require payment (except in open mode)
            (DetectedProtocol::Unknown, MeshMode::Open) => {
                trace!("Unknown protocol in open mode → Free routing");
                RoutingPolicy::Free
            }
            (DetectedProtocol::Unknown, _) => {
                trace!("Unknown protocol → Payment required");
                RoutingPolicy::PaymentRequired
            }
        }
    }

    /// Check if command is a known Bitcoin P2P command
    fn is_bitcoin_command(&self, command: &str) -> bool {
        // Core Bitcoin P2P commands
        matches!(command, 
            "version" | "verack" | "ping" | "pong" |
            "getheaders" | "headers" | "getblocks" | "block" |
            "getdata" | "inv" | "tx" | "notfound" |
            "getaddr" | "addr" | "mempool" | "feefilter" |
            "sendheaders" | "sendcmpct" | "cmpctblock" |
            "getblocktxn" | "blocktxn" | "getcfilters" |
            "cfilter" | "getcfheaders" | "cfheaders" |
            "getcfcheckpt" | "cfcheckpt" | "sendpkgtxn" |
            "pkgtxn" | "pkgtxnreject"
        )
    }

    /// Check if command is a Commons governance command
    fn is_governance_command(&self, command: &str) -> bool {
        // Commons governance messages
        matches!(command,
            "econreg" | "econveto" | "econstat" | "econfork" |
            "getbanlist" | "banlist"
        )
    }

    /// Check if message is Stratum V2 protocol
    fn is_stratum_v2_message(&self, message: &[u8]) -> bool {
        // Stratum V2 uses specific message type tags
        // This is a simplified check - in production, would parse the protocol properly
        // Stratum V2 messages typically start with a message type tag (u16)
        if message.len() >= 2 {
            let tag = u16::from_le_bytes([message[0], message[1]]);
            // Stratum V2 message type tags are in specific ranges
            // This is a placeholder - would need actual Stratum V2 protocol parsing
            (0x0100..=0x01FF).contains(&tag) || (0x0200..=0x02FF).contains(&tag)
        } else {
            false
        }
    }

    /// Get current mesh mode
    pub fn mode(&self) -> MeshMode {
        self.mode
    }

    /// Set mesh mode (for runtime configuration changes)
    pub fn set_mode(&mut self, mode: MeshMode) {
        debug!("Routing policy mode changed: {:?} → {:?}", self.mode, mode);
        self.mode = mode;
    }
}

/// Convert string mode to MeshMode enum
impl From<&str> for MeshMode {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "bitcoin_only" | "bitcoin-only" => MeshMode::BitcoinOnly,
            "payment_gated" | "payment-gated" | "paymentgated" => MeshMode::PaymentGated,
            "open" => MeshMode::Open,
            _ => {
                debug!("Unknown mesh mode '{}', defaulting to payment_gated", s);
                MeshMode::PaymentGated
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitcoin_p2p_detection() {
        let engine = RoutingPolicyEngine::new(MeshMode::PaymentGated);
        
        // Bitcoin mainnet magic + version command
        let bitcoin_message = vec![
            0xf9, 0xbe, 0xb4, 0xd9, // magic
            0x76, 0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x00, // "version\0"
            0x00, 0x00, 0x00, 0x00, // payload length
            0x00, 0x00, 0x00, 0x00, // checksum
        ];
        
        let protocol = engine.detect_protocol(&bitcoin_message);
        assert_eq!(protocol, DetectedProtocol::BitcoinP2P);
        
        let policy = engine.determine_policy(protocol);
        assert_eq!(policy, RoutingPolicy::Free);
    }

    #[test]
    fn test_mesh_packet_detection() {
        let engine = RoutingPolicyEngine::new(MeshMode::PaymentGated);
        
        // Mesh packet magic
        let mesh_message = vec![
            b'M', b'E', b'S', b'H', // mesh magic
            0x00, 0x00, 0x00, 0x00, // rest of header
        ];
        
        let protocol = engine.detect_protocol(&mesh_message);
        assert_eq!(protocol, DetectedProtocol::MeshPacket);
        
        let policy = engine.determine_policy(protocol);
        assert_eq!(policy, RoutingPolicy::PaymentRequired);
    }

    #[test]
    fn test_unknown_protocol() {
        let engine = RoutingPolicyEngine::new(MeshMode::PaymentGated);
        
        // Random data
        let unknown_message = vec![0x12, 0x34, 0x56, 0x78];
        
        let protocol = engine.detect_protocol(&unknown_message);
        assert_eq!(protocol, DetectedProtocol::Unknown);
        
        let policy = engine.determine_policy(protocol);
        assert_eq!(policy, RoutingPolicy::PaymentRequired);
    }

    #[test]
    fn test_open_mode() {
        let engine = RoutingPolicyEngine::new(MeshMode::Open);
        
        // Unknown protocol in open mode should be free
        let protocol = DetectedProtocol::Unknown;
        let policy = engine.determine_policy(protocol);
        assert_eq!(policy, RoutingPolicy::Free);
    }
}

