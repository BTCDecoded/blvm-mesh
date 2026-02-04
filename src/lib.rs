//! Commons Mesh networking module for blvm-node

pub mod api;
pub mod client;
pub mod client_api;
pub mod discovery;
pub mod error;
pub mod manager;
pub mod network;
pub mod nodeapi_ipc;
pub mod packet;
pub mod payment_proof;
pub mod replay;
pub mod routing;
pub mod routing_policy;
pub mod verifier;

// Re-export commonly used types
pub use api::{
    DiscoverRouteRequest, DiscoverRouteResponse, MeshModuleAPI, RegisterProtocolRequest,
    RegisterProtocolResponse, SendPacketRequest, SendPacketResponse,
};
pub use client_api::MeshClient;
pub use manager::{MeshManager, MeshStats};
pub use packet::{MeshPacket, PacketType};
pub use payment_proof::{PaymentProof, VerificationResult};
pub use routing::NodeId;
pub use routing_policy::{MeshMode, RoutingPolicy, RoutingPolicyEngine};
