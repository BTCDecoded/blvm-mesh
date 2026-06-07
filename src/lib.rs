//! Commons Mesh networking module for blvm-node

pub mod api;
pub mod client_api;
pub mod app_transport;
pub mod config;
pub mod discovery;
pub mod edge_transport;
pub mod identity;
pub mod module;
pub mod storage;

pub use config::{MeshConfig, MeshPeerEntry};
pub use identity::{MeshIdentity, PROTOCOL_DISCOVERY, PROTOCOL_HELLO};
pub mod error;
pub mod manager;
pub mod network;
pub mod packet;
pub mod packet_sequence;
pub mod payment_proof;
pub mod rate_limit;
pub mod replay;
pub mod routing;
pub mod routing_policy;
pub mod verifier;

#[doc(hidden)]
pub mod test_support;

// Re-export commonly used types
pub use api::{
    DiscoverRouteRequest, DiscoverRouteResponse, MeshModuleAPI, PeerEntry, RegisterProtocolRequest,
    RegisterProtocolResponse, SendPacketRequest, SendPacketResponse,
};
pub use app_transport::MeshAppTransport;
pub use client_api::MeshClient;
pub use edge_transport::{
    chunk_bytes, parse_edge_transport, EdgeTransportKind, MESHTASTIC_LORA_APP_PAYLOAD_PLANNING_MAX,
};
pub use manager::{MeshManager, MeshStats};
pub use module::MeshModule;
pub use packet::{MeshPacket, PacketType};
pub use payment_proof::{PaymentProof, VerificationResult};
pub use routing::NodeId;
pub use routing_policy::{MeshMode, RoutingPolicy, RoutingPolicyEngine};
