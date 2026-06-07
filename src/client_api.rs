//! Client helper for modules that depend on blvm-mesh
//!
//! Provides a convenient API for submodules to call blvm-mesh ModuleAPI via IPC.

use crate::api::{
    DiscoverRouteRequest, DiscoverRouteResponse, PeerEntry, RegisterProtocolRequest,
    SendPacketRequest, SendPacketResponse,
};
use crate::app_transport::MeshAppTransport;
use crate::payment_proof::PaymentProof;
use crate::routing::NodeId;
use async_trait::async_trait;
use blvm_node::module::traits::{ModuleError, NodeAPI};
use std::sync::Arc;

/// Client helper for modules that depend on blvm-mesh
#[derive(Clone)]
pub struct MeshClient {
    node_api: Arc<dyn NodeAPI>,
    mesh_module_id: String,
}

impl MeshClient {
    /// Create a new mesh client
    pub fn new(node_api: Arc<dyn NodeAPI>, mesh_module_id: String) -> Self {
        Self {
            node_api,
            mesh_module_id,
        }
    }

    /// Planning budget (bytes) for **one LoRa-side frame** when bridging Meshtastic-style radios.
    /// Larger payloads should be split before wrapping in mesh/bridge envelopes; see
    /// [`crate::edge_transport`](edge_transport).
    #[inline]
    pub fn meshtastic_lora_payload_budget_bytes() -> usize {
        crate::edge_transport::MESHTASTIC_LORA_APP_PAYLOAD_PLANNING_MAX
    }

    /// Split `data` into chunks suitable for [`Self::meshtastic_lora_payload_budget_bytes`].
    #[inline]
    pub fn chunk_for_meshtastic_lora<'a>(data: &'a [u8]) -> impl Iterator<Item = &'a [u8]> + 'a {
        crate::edge_transport::chunk_bytes(data, Self::meshtastic_lora_payload_budget_bytes())
    }

    /// Send a packet through the mesh
    pub async fn send_packet(
        &self,
        _caller_module_id: &str,
        destination: NodeId,
        payload: Vec<u8>,
        payment_proof: Option<PaymentProof>,
        protocol_id: Option<String>,
    ) -> Result<SendPacketResponse, ModuleError> {
        let request = SendPacketRequest {
            destination,
            payload,
            payment_proof,
            protocol_id,
            ttl: Some(3600),
        };

        let params = bincode::serialize(&request)
            .map_err(|e| ModuleError::OperationError(format!("Serialization failed: {e}")))?;

        let response = self
            .node_api
            .call_module(Some(&self.mesh_module_id), "send_packet", params)
            .await?;

        if response.len() > crate::packet::MAX_BINCODE_PAYLOAD_SIZE {
            return Err(ModuleError::OperationError(format!(
                "Response too large: {} bytes (max: {} bytes)",
                response.len(),
                crate::packet::MAX_BINCODE_PAYLOAD_SIZE
            )));
        }
        let result: SendPacketResponse = bincode::deserialize(&response)
            .map_err(|e| ModuleError::OperationError(format!("Deserialization failed: {e}")))?;

        Ok(result)
    }

    /// Discover a route to a destination
    pub async fn discover_route(
        &self,
        _caller_module_id: &str,
        destination: NodeId,
        max_hops: Option<u8>,
    ) -> Result<DiscoverRouteResponse, ModuleError> {
        let request = DiscoverRouteRequest {
            destination,
            max_hops,
            timeout_seconds: Some(30),
        };

        let params = bincode::serialize(&request)
            .map_err(|e| ModuleError::OperationError(format!("Serialization failed: {e}")))?;

        let response = self
            .node_api
            .call_module(Some(&self.mesh_module_id), "discover_route", params)
            .await?;

        if response.len() > crate::packet::MAX_BINCODE_PAYLOAD_SIZE {
            return Err(ModuleError::OperationError(format!(
                "Response too large: {} bytes (max: {} bytes)",
                response.len(),
                crate::packet::MAX_BINCODE_PAYLOAD_SIZE
            )));
        }
        let result: DiscoverRouteResponse = bincode::deserialize(&response)
            .map_err(|e| ModuleError::OperationError(format!("Deserialization failed: {e}")))?;

        Ok(result)
    }

    /// Register a protocol handler
    ///
    /// **Deprecated:** subscribe to `MeshPacketReceived` and filter on
    /// `packet.metadata.protocol` instead. Kept for existing submodules.
    #[deprecated(note = "use MeshPacketReceived events and MeshAppTransport instead")]
    pub async fn register_protocol_handler(
        &self,
        _caller_module_id: &str,
        protocol_id: String,
        handler_method: String,
    ) -> Result<(), ModuleError> {
        let request = RegisterProtocolRequest {
            protocol_id,
            handler_method,
        };

        let params = bincode::serialize(&request)
            .map_err(|e| ModuleError::OperationError(format!("Serialization failed: {e}")))?;

        self.node_api
            .call_module(
                Some(&self.mesh_module_id),
                "register_protocol_handler",
                params,
            )
            .await?;

        Ok(())
    }

    /// Get node ID from mesh
    pub async fn get_node_id(&self) -> Result<NodeId, ModuleError> {
        let params = vec![]; // No parameters needed

        let response = self
            .node_api
            .call_module(Some(&self.mesh_module_id), "get_node_id", params)
            .await?;

        if response.len() > crate::packet::MAX_BINCODE_PAYLOAD_SIZE {
            return Err(ModuleError::OperationError(format!(
                "Response too large: {} bytes (max: {} bytes)",
                response.len(),
                crate::packet::MAX_BINCODE_PAYLOAD_SIZE
            )));
        }
        let node_id: NodeId = bincode::deserialize(&response)
            .map_err(|e| ModuleError::OperationError(format!("Deserialization failed: {e}")))?;

        Ok(node_id)
    }

    /// List direct mesh peers.
    pub async fn get_peer_list(&self) -> Result<Vec<PeerEntry>, ModuleError> {
        let response = self
            .node_api
            .call_module(
                Some(&self.mesh_module_id),
                "get_peer_list",
                Vec::new(),
            )
            .await?;
        bincode::deserialize(&response)
            .map_err(|e| ModuleError::OperationError(format!("Deserialization failed: {e}")))
    }

    /// Estimate route fee in satoshis.
    pub async fn quote_route_fee(
        &self,
        destination: NodeId,
        base_fee_sats: u64,
    ) -> Result<u64, ModuleError> {
        #[derive(serde::Serialize)]
        struct QuoteRequest {
            destination: NodeId,
            base_fee_sats: u64,
        }
        let params = bincode::serialize(&QuoteRequest {
            destination,
            base_fee_sats,
        })
        .map_err(|e| ModuleError::OperationError(format!("Serialization failed: {e}")))?;
        let response = self
            .node_api
            .call_module(Some(&self.mesh_module_id), "quote_route_fee", params)
            .await?;
        bincode::deserialize(&response)
            .map_err(|e| ModuleError::OperationError(format!("Deserialization failed: {e}")))
    }
}

#[async_trait]
impl MeshAppTransport for MeshClient {
    async fn send_packet(
        &self,
        destination: NodeId,
        payload: Vec<u8>,
        payment_proof: Option<PaymentProof>,
        protocol_id: Option<String>,
    ) -> Result<SendPacketResponse, ModuleError> {
        self.send_packet("", destination, payload, payment_proof, protocol_id)
            .await
    }

    async fn discover_route(
        &self,
        destination: NodeId,
        max_hops: Option<u8>,
    ) -> Result<DiscoverRouteResponse, ModuleError> {
        self.discover_route("", destination, max_hops).await
    }

    async fn get_node_id(&self) -> Result<NodeId, ModuleError> {
        MeshClient::get_node_id(self).await
    }

    async fn get_peer_list(&self) -> Result<Vec<PeerEntry>, ModuleError> {
        MeshClient::get_peer_list(self).await
    }

    async fn quote_route_fee(
        &self,
        destination: NodeId,
        base_fee_sats: u64,
    ) -> Result<u64, ModuleError> {
        MeshClient::quote_route_fee(self, destination, base_fee_sats).await
    }
}
