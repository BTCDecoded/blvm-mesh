//! Generic application transport trait for mesh clients.

use crate::api::{DiscoverRouteResponse, PeerEntry, SendPacketResponse};
use crate::payment_proof::PaymentProof;
use crate::routing::NodeId;
use async_trait::async_trait;
use blvm_node::module::traits::ModuleError;

/// Application-facing mesh transport API.
///
/// Prefer this trait over raw `call_module` for app modules. `MeshClient` is the
/// reference implementation; inbound delivery uses `MeshPacketReceived` events.
#[async_trait]
pub trait MeshAppTransport: Send + Sync {
    async fn send_packet(
        &self,
        destination: NodeId,
        payload: Vec<u8>,
        payment_proof: Option<PaymentProof>,
        protocol_id: Option<String>,
    ) -> Result<SendPacketResponse, ModuleError>;

    async fn discover_route(
        &self,
        destination: NodeId,
        max_hops: Option<u8>,
    ) -> Result<DiscoverRouteResponse, ModuleError>;

    async fn get_node_id(&self) -> Result<NodeId, ModuleError>;

    async fn get_peer_list(&self) -> Result<Vec<PeerEntry>, ModuleError>;

    async fn quote_route_fee(
        &self,
        destination: NodeId,
        base_fee_sats: u64,
    ) -> Result<u64, ModuleError>;
}
