//! Commons Mesh networking module for blvm-node

pub mod api;
pub mod client_api;
pub mod module;
pub mod config;
pub mod discovery;
pub mod storage;

pub use config::MeshConfig;
pub mod error;
pub mod manager;
pub mod network;
pub mod packet;
pub mod payment_proof;
pub mod replay;
pub mod routing;
pub mod routing_policy;
pub mod verifier;

// Re-export commonly used types
pub use api::{
    DiscoverRouteRequest, DiscoverRouteResponse, MeshModuleAPI, PeerEntry,
    RegisterProtocolRequest, RegisterProtocolResponse, SendPacketRequest, SendPacketResponse,
};
pub use client_api::MeshClient;
pub use manager::{MeshManager, MeshStats};
pub use module::MeshModule;
pub use packet::{MeshPacket, PacketType};
pub use payment_proof::{PaymentProof, VerificationResult};
pub use routing::NodeId;
pub use routing_policy::{MeshMode, RoutingPolicy, RoutingPolicyEngine};
