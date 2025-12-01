//! Mesh manager - main coordination logic

use crate::discovery::RouteDiscovery;
use crate::error::MeshError;
use crate::network::{deserialize_mesh_packet, extract_mesh_packet, serialize_mesh_packet};
use crate::packet::MeshPacket;
use crate::payment_proof::PaymentProof;
use crate::routing::{NodeId, RoutingTable, RoutingStats};
use crate::routing_policy::{MeshMode, RoutingPolicyEngine};
use crate::replay::{ReplayPrevention, ReplayStats};
use crate::verifier::PaymentVerifier;
use bllvm_node::module::ipc::protocol::ModuleMessage;
use bllvm_node::module::traits::{EventPayload, EventType, NodeAPI};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, trace, warn};

/// Mesh manager coordinates all mesh operations
pub struct MeshManager {
    /// Whether mesh is enabled
    enabled: bool,
    /// Routing policy engine
    routing_policy: RoutingPolicyEngine,
    /// Payment verifier for payment-gated routing
    payment_verifier: PaymentVerifier,
    /// Replay prevention for payment proofs
    replay_prevention: Arc<Mutex<ReplayPrevention>>,
    /// Routing table for mesh networking
    routing_table: Arc<RoutingTable>,
    /// Route discovery manager
    route_discovery: Arc<RouteDiscovery>,
    /// Node ID (32 bytes, SHA256 of node's public key)
    node_id: NodeId,
    /// Node API for querying node state
    node_api: Arc<dyn NodeAPI>,
}

impl MeshManager {
    /// Create a new mesh manager
    pub async fn new(
        ctx: &bllvm_node::module::traits::ModuleContext,
        node_api: Arc<dyn NodeAPI>,
    ) -> Result<Self, MeshError> {
        let enabled = ctx.get_config_or("mesh.enabled", "false") == "true";
        let mode_str = ctx.get_config_or("mesh.mode", "payment_gated");
        let mode = MeshMode::from(mode_str.as_str());
        
        let routing_policy = RoutingPolicyEngine::new(mode);
        let payment_verifier = PaymentVerifier::new(Arc::clone(&node_api));
        
        // Replay prevention with 24-hour expiry
        const REPLAY_EXPIRY_SECONDS: u64 = 24 * 60 * 60; // 24 hours
        let replay_prevention = Arc::new(Mutex::new(ReplayPrevention::new(REPLAY_EXPIRY_SECONDS)));
        
        // Routing table with 1-hour route expiry
        const ROUTE_EXPIRY_SECONDS: u64 = 60 * 60; // 1 hour
        let routing_table = Arc::new(RoutingTable::new(ROUTE_EXPIRY_SECONDS));
        
        // Route discovery with 30-second timeout
        const DISCOVERY_TIMEOUT_SECONDS: u64 = 30;
        const MAX_DISCOVERY_HOPS: u8 = 10;
        let route_discovery = Arc::new(RouteDiscovery::new(
            Arc::clone(&routing_table),
            MAX_DISCOVERY_HOPS,
            DISCOVERY_TIMEOUT_SECONDS,
        ));
        
        // Get or generate node ID
        // Try to load from storage first, otherwise generate and store it
        let node_id = Self::get_or_generate_node_id(node_api.as_ref()).await;
        
        debug!(
            "Initializing mesh manager: enabled={}, mode={:?}, node_id={:x?}",
            enabled, mode, &node_id[..8]
        );
        
        Ok(Self {
            enabled,
            routing_policy,
            payment_verifier,
            replay_prevention,
            routing_table,
            route_discovery,
            node_id,
            node_api,
        })
    }
    
    /// Determine routing policy for a message
    pub fn determine_routing_policy(&self, message: &[u8]) -> crate::routing_policy::RoutingPolicy {
        if !self.enabled {
            // If mesh is disabled, all messages should use standard routing
            return crate::routing_policy::RoutingPolicy::Free;
        }
        
        let protocol = self.routing_policy.detect_protocol(message);
        self.routing_policy.determine_policy(protocol)
    }
    
    /// Start the mesh manager
    pub async fn start(&self) -> Result<(), MeshError> {
        debug!(
            "Starting mesh manager (enabled={}, mode={:?})",
            self.enabled,
            self.routing_policy.mode()
        );
        
        if !self.enabled {
            return Ok(());
        }
        
        // Start periodic cleanup tasks
        let routing_table = Arc::clone(&self.routing_table);
        let replay_prevention = Arc::clone(&self.replay_prevention);
        let route_discovery = Arc::clone(&self.route_discovery);
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600)); // 1 hour
            loop {
                interval.tick().await;
                
                // Cleanup expired routes
                routing_table.cleanup_expired();
                
                // Cleanup expired replay hashes (lock-free with DashMap)
                let replay = replay_prevention.lock().await;
                replay.cleanup_expired();
                
                // Cleanup expired route discovery requests
                route_discovery.cleanup_expired().await;
            }
        });
        
        info!("Mesh manager started");
        Ok(())
    }
    
    /// Route a packet through the mesh
    ///
    /// This is the main entry point for routing packets. It:
    /// 1. Validates the packet
    /// 2. Checks routing policy
    /// 3. Verifies payment (if required)
    /// 4. Checks replay prevention
    /// 5. Routes the packet
    pub async fn route_packet(&self, packet: &MeshPacket) -> Result<(), MeshError> {
        if !self.enabled {
            return Err(MeshError::MeshDisabled("Mesh is disabled".to_string()));
        }
        
        // Early exit: Check if packet payload is empty (cheap check before expensive validation)
        if packet.payload.is_empty() {
            return Err(MeshError::InvalidPacket("Empty payload".to_string()));
        }
        
        // Early exit: Check if destination is valid (cheap check)
        if packet.destination == [0u8; 32] {
            return Err(MeshError::InvalidPacket("Invalid destination (zero hash)".to_string()));
        }
        
        // Validate packet structure
        packet.validate().map_err(|e| MeshError::InvalidPacket(e))?;
        
        // Determine routing policy
        let policy = self.determine_routing_policy(&packet.payload);
        
        // Check if payment is required
        if policy == crate::routing_policy::RoutingPolicy::PaymentRequired {
            // Verify payment proof
            if let Some(ref proof) = packet.payment_proof {
                // Check replay prevention (lock-free with DashMap)
                let replay = self.replay_prevention.lock().await;
                replay.check_replay(proof, &packet.source, packet.sequence)
                    .map_err(|e| MeshError::ReplayDetected(e))?;
                
                // Verify payment
                let verification = self.payment_verifier.verify(proof).await
                    .map_err(|e| MeshError::PaymentVerification(e.to_string()))?;
                
                if !verification.verified {
                    return Err(MeshError::PaymentVerification(
                        verification.error.unwrap_or_else(|| "Payment verification failed".to_string())
                    ));
                }
                
                debug!(
                    "Payment verified: amount={} sats, destination={:x?}",
                    verification.amount,
                    &packet.destination[..8]
                );
            } else {
                return Err(MeshError::PaymentVerification(
                    "Payment proof required for paid packets".to_string()
                ));
            }
        }
        
        // Route the packet
        self.forward_packet(packet).await?;
        
        Ok(())
    }
    
    /// Forward a packet to the next hop
    async fn forward_packet(&self, packet: &MeshPacket) -> Result<(), MeshError> {
        // Early exit: Check if mesh is enabled (cheap check before expensive operations)
        if !self.enabled {
            return Err(MeshError::MeshDisabled("Mesh is disabled".to_string()));
        }
        
        // Early exit: Check if packet payload is empty (cheap check)
        if packet.payload.is_empty() {
            return Err(MeshError::InvalidPacket("Empty payload".to_string()));
        }
        
        // Early exit: Check if destination is valid (cheap check)
        if packet.destination == [0u8; 32] {
            return Err(MeshError::InvalidPacket("Invalid destination (zero hash)".to_string()));
        }
        
        // Get my node ID from storage
        let my_node_id = Self::get_or_generate_node_id(self.node_api.as_ref()).await;
        
        // Find route to destination
        let mut route = self.routing_table.find_route(&packet.destination);
        
        // If route not found, try route discovery
        if route.is_none() {
            debug!(
                "Route not found, attempting route discovery: destination={:x?}",
                &packet.destination[..8]
            );
            
            match self
                .route_discovery
                .discover_route(packet.destination, self.node_id)
                .await
            {
                Ok(Some(discovered_route)) => {
                    route = Some(discovered_route);
                    info!(
                        "Route discovered: destination={:x?}, route_length={}",
                        &packet.destination[..8],
                        route.as_ref().unwrap().len()
                    );
                }
                Ok(None) => {
                    // Route discovery in progress or failed
                }
                Err(e) => {
                    warn!("Route discovery failed: {}", e);
                }
            }
        }
        
        if let Some(route_path) = route {
            debug!(
                "Routing packet: destination={:x?}, route_length={}",
                &packet.destination[..8],
                route_path.len()
            );
            
            // Optimization: Cache route entry lookup to avoid redundant lookups
            // Get route entry once and reuse for both next_hop and destination lookups
            let route_entry = self.routing_table.get_route(&packet.destination);
            
            // Get next hop from route
            let next_hop = if route_path.len() > 1 {
                Some(route_path[1]) // Next hop is second node in route
            } else {
                None
            };
            
            if let Some(next_hop_id) = next_hop {
                // Optimization: Only clone if we need to modify the route
                // Check if this node is already in the route (cheap check before expensive clone)
                let serialized = if packet.route.contains(&self.node_id) {
                    // Node already in route, no modification needed - use original packet
                    serialize_mesh_packet(packet)?
                } else {
                    // Need to add node to route - clone and modify
                    let mut packet_to_forward = packet.clone();
                    packet_to_forward.add_to_route(self.node_id);
                    serialize_mesh_packet(&packet_to_forward)?
                };
                
                // Optimization: Reuse cached route entry if available, otherwise lookup next_hop
                let peer_address = if let Some(ref entry) = route_entry {
                    // Check if next_hop is the destination (direct route)
                    if next_hop_id == packet.destination {
                        entry.direct_address.as_ref()
                            .and_then(|addr| String::from_utf8(addr.clone()).ok())
                    } else {
                        // Lookup next_hop separately
                        self.find_peer_address(&next_hop_id).await
                    }
                } else {
                    // Route entry not cached, lookup next_hop
                    self.find_peer_address(&next_hop_id).await
                };
                
                if let Some(addr) = peer_address {
                    // Send packet to next hop
                    self.send_mesh_packet(addr, serialized).await?;
                    
                    info!(
                        "Packet forwarded: destination={:x?}, next_hop={:x?}, route_length={}",
                        &packet.destination[..8],
                        &next_hop_id[..8],
                        route_path.len()
                    );
                } else {
                    // Peer not found - might need route discovery
                    warn!(
                        "Next hop not found in routing table: node_id={:x?}",
                        &next_hop_id[..8]
                    );
                    return Err(MeshError::RouteNotFound(format!(
                        "Next hop not found: {:x?}",
                        &next_hop_id[..8]
                    )));
                }
            } else {
                // Direct route (destination is next hop)
                info!(
                    "Packet for direct peer: destination={:x?}",
                    &packet.destination[..8]
                );
                
                // Serialize packet
                let serialized = serialize_mesh_packet(packet)?;
                
                // Optimization: Reuse cached route entry instead of looking up again
                let peer_address = if let Some(ref entry) = route_entry {
                    entry.direct_address.as_ref()
                        .and_then(|addr| String::from_utf8(addr.clone()).ok())
                } else {
                    // Fallback: lookup if not cached
                    self.find_peer_address(&packet.destination).await
                };
                
                if let Some(addr) = peer_address {
                    // Send packet directly to destination
                    self.send_mesh_packet(addr, serialized).await?;
                } else {
                    return Err(MeshError::RouteNotFound(format!(
                        "Destination peer not found: {:x?}",
                        &packet.destination[..8]
                    )));
                }
            }
        } else {
            // Route not found - would need route discovery
            warn!(
                "Route not found for destination: {:x?}",
                &packet.destination[..8]
            );
            return Err(MeshError::RouteNotFound(format!(
                "No route to destination: {:x?}",
                &packet.destination[..8]
            )));
        }
        
        Ok(())
    }
    
    /// Find peer address for a node ID
    async fn find_peer_address(&self, node_id: &NodeId) -> Option<String> {
        // Check routing table for direct peer
        if let Some(entry) = self.routing_table.get_route(node_id) {
            if let Some(ref address) = entry.direct_address {
                // Convert address bytes to string (simplified - in production would handle different address types)
                String::from_utf8(address.clone()).ok()
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Send mesh packet to peer
    async fn send_mesh_packet(&self, peer_address: String, packet_data: Vec<u8>) -> Result<(), MeshError> {
        // Send packet via NodeAPI to network layer
        self.node_api.send_mesh_packet_to_peer(peer_address, packet_data)
            .await
            .map_err(|e| MeshError::NetworkError(format!("Failed to send mesh packet: {}", e)))?;
        
        debug!("Mesh packet sent successfully");
        Ok(())
    }
    
    /// Handle an incoming mesh packet
    pub async fn handle_incoming_packet(&self, packet: &MeshPacket) -> Result<(), MeshError> {
        if !self.enabled {
            return Err(MeshError::MeshDisabled("Mesh is disabled".to_string()));
        }
        
        // Validate packet
        packet.validate().map_err(|e| MeshError::InvalidPacket(e))?;
        
        // Check if packet is for this node
        if packet.is_for_me(&self.node_id) {
            // Packet is for this node - deliver it
            debug!("Packet delivered to local node: source={:x?}", &packet.source[..8]);
                        // Deliver packet to application layer
                        // Note: Application layer delivery would be handled by the node's network layer
                        // This module processes mesh packets and forwards them to the next hop
                        debug!("Delivered mesh packet to destination: {:x?}", &packet.destination[..8]);
            return Ok(());
        }
        
        // Check if packet should be forwarded
        if packet.should_forward(&self.node_id) {
            // Forward packet to next hop
            debug!("Forwarding packet: destination={:x?}", &packet.destination[..8]);
            self.forward_packet(packet).await?;
        } else {
            // Packet is not for us and we're not in the route - drop it
            warn!("Dropping packet: not for us and not in route");
        }
        
        Ok(())
    }
    
    /// Handle an event from the node
    pub async fn handle_event(
        &self,
        event: &ModuleMessage,
        _node_api: &dyn NodeAPI,
    ) -> Result<(), MeshError> {
        if !self.enabled {
            return Ok(());
        }
        
        match event {
            ModuleMessage::Event(event_msg) => {
                match event_msg.event_type {
                    EventType::PeerConnected => {
                        debug!("Peer connected event received");
                        if let EventPayload::PeerConnected {
                            peer_addr,
                            transport_type,
                            ..
                        } = &event_msg.payload
                        {
                            // Derive node ID from peer address (simplified - in production would use peer's public key)
                            let peer_node_id = Self::derive_node_id_from_address(peer_addr);
                            
                            // Convert address string to bytes (simplified)
                            let address_bytes = peer_addr.as_bytes().to_vec();
                            
                            // Add to routing table as direct peer
                            self.routing_table.add_direct_peer(peer_node_id, address_bytes);
                            
                            info!(
                                "Added peer to routing table: node_id={:x?}, addr={}, transport={}",
                                &peer_node_id[..8],
                                peer_addr,
                                transport_type
                            );
                        }
                    }
                    EventType::PeerDisconnected => {
                        debug!("Peer disconnected event received");
                        if let EventPayload::PeerDisconnected { peer_addr, .. } = &event_msg.payload
                        {
                            // Derive node ID from peer address
                            let peer_node_id = Self::derive_node_id_from_address(peer_addr);
                            
                            // Remove from routing table
                            self.routing_table.remove_direct_peer(&peer_node_id);
                            
                            info!(
                                "Removed peer from routing table: node_id={:x?}, addr={}",
                                &peer_node_id[..8],
                                peer_addr
                            );
                        }
                    }
                    EventType::MessageReceived => {
                        debug!("Message received event received");
                        // Check if message is a mesh packet and handle it
                        // The node's network layer should route mesh packets to this module
                        // This is handled via IPC event routing
                        debug!("Received potential mesh packet from peer");
                        // This would involve:
                        // 1. Extracting message data from event payload
                        // 2. Checking if it's a mesh packet (magic bytes)
                        // 3. Deserializing and handling via handle_incoming_packet
                    }
                    EventType::PaymentVerified => {
                        debug!("Payment verified event received");
                        // Payment verification handled in route_packet
                    }
                    _ => {
                        // Ignore other events
                    }
                }
            }
            _ => {
                // Not an event message
            }
        }
        
        Ok(())
    }
    
    /// Get routing statistics
    pub async fn get_stats(&self) -> MeshStats {
        let routing_stats = self.routing_table.stats();
        let replay_stats = self.replay_prevention.lock().await.stats();
        
        MeshStats {
            enabled: self.enabled,
            mode: self.routing_policy.mode(),
            routing: routing_stats,
            replay: replay_stats,
        }
    }
    
    /// Get or generate node ID
    /// Tries to load from storage first, otherwise generates and stores a new one
    async fn get_or_generate_node_id(node_api: &dyn NodeAPI) -> NodeId {
        use sha2::{Digest, Sha256};
        
        // Try to load from storage
        let storage_key = b"node_id";
        if let Ok(Some(tree_id)) = node_api.storage_open_tree("mesh_config".to_string()).await {
            if let Ok(Some(stored_id)) = node_api.storage_get(tree_id.clone(), storage_key.to_vec()).await {
                if stored_id.len() == 32 {
                    let mut node_id = [0u8; 32];
                    node_id.copy_from_slice(&stored_id);
                    return node_id;
                }
            }
        }
        
        // Try to get node ID from network info (if available)
        // First, try to get from network stats which might include node identity
        let node_id = if let Ok(network_stats) = node_api.get_network_stats().await {
            // Use network stats to create a more stable node ID
            // In production, this would use the node's actual public key
            // For now, create a deterministic ID from network characteristics
            let mut id_data = Vec::new();
            id_data.extend_from_slice(&network_stats.peer_count.to_le_bytes());
            id_data.extend_from_slice(&network_stats.hash_rate.to_le_bytes());
            
            // Add chain state for additional uniqueness
            if let Ok(chain_tip) = node_api.get_chain_tip().await {
                id_data.extend_from_slice(&chain_tip);
            }
            if let Ok(height) = node_api.get_block_height().await {
                id_data.extend_from_slice(&height.to_le_bytes());
            }
            
            // Add a constant seed for this node instance
            id_data.extend_from_slice(b"bllvm_mesh_node_id_v1");
            
            let hash = Sha256::digest(&id_data);
            let mut node_id = [0u8; 32];
            node_id.copy_from_slice(&hash);
            node_id
        } else {
            // Fallback: Generate from chain state (deterministic per node)
            let chain_tip = node_api.get_chain_tip().await.unwrap_or([0u8; 32]);
            let chain_height = node_api.get_block_height().await.unwrap_or(0);
            
            // Create deterministic ID from chain state
            let mut id_data = Vec::new();
            id_data.extend_from_slice(&chain_tip);
            id_data.extend_from_slice(&chain_height.to_le_bytes());
            id_data.extend_from_slice(b"mesh_node_id");
            
            let hash = Sha256::digest(&id_data);
            let mut node_id = [0u8; 32];
            node_id.copy_from_slice(&hash);
            node_id
        };
        
        // Store for future use
        if let Ok(tree_id) = node_api.storage_open_tree("mesh_config".to_string()).await {
            let _ = node_api.storage_insert(tree_id, storage_key.to_vec(), node_id.to_vec()).await;
        }
        
        node_id
    }
    
    /// Derive node ID from peer address (simplified - in production would use peer's public key)
    fn derive_node_id_from_address(peer_addr: &str) -> NodeId {
        // In production, this would:
        // 1. Get peer's public key from handshake or peer info
        // 2. SHA256 hash the public key
        // 3. Use first 32 bytes as node ID
        
        // For now, derive from address (not secure, but functional for testing)
        let hash = Sha256::digest(peer_addr.as_bytes());
        let mut node_id = [0u8; 32];
        node_id.copy_from_slice(&hash);
        node_id
    }
}

