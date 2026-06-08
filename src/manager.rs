//! Mesh manager - main coordination logic

use crate::config::MeshConfig;
use crate::discovery::{DiscoveryMessage, DiscoveryStart, RouteDiscovery};
use crate::error::MeshError;
use crate::identity::{MeshIdentity, PROTOCOL_DISCOVERY, PROTOCOL_HELLO};
use crate::network::{deserialize_mesh_packet, serialize_mesh_packet};
use crate::packet::{MeshPacket, PacketMetadata, PacketType};
use crate::packet_sequence::PacketSequenceGuard;
use crate::rate_limit::RateLimiter;
use crate::replay::{ReplayPrevention, ReplayStats};
use crate::routing::{NodeId, RoutingEntry, RoutingStats, RoutingTable};
use crate::routing_policy::{MeshMode, RoutingPolicyEngine};
use crate::verifier::PaymentVerifier;
use blvm_node::module::ipc::protocol::{EventPayload, ModuleMessage};
use blvm_node::module::traits::{EventType, NodeAPI};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Mesh statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshStats {
    pub enabled: bool,
    pub mode: MeshMode,
    pub routing: RoutingStats,
    pub replay: ReplayStats,
}

/// App payload delivered locally (for BitSov and other mesh consumers).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalDelivery {
    pub protocol_id: String,
    pub payload: Vec<u8>,
    pub source: NodeId,
}

const LOCAL_DELIVERY_QUEUE_CAP: usize = 256;

/// Mesh manager coordinates all mesh operations
pub struct MeshManager {
    /// Whether mesh is enabled
    enabled: bool,
    /// Routing policy engine (RwLock for runtime set_mode)
    routing_policy: RwLock<RoutingPolicyEngine>,
    /// Payment verifier for payment-gated routing
    payment_verifier: PaymentVerifier,
    /// Replay prevention for payment proofs
    replay_prevention: Arc<Mutex<ReplayPrevention>>,
    /// Routing table for mesh networking
    routing_table: Arc<RoutingTable>,
    /// Route discovery manager
    route_discovery: Arc<RouteDiscovery>,
    /// Ed25519 identity (D-1)
    identity: Arc<MeshIdentity>,
    /// Node ID (32-byte Ed25519 verifying key)
    node_id: NodeId,
    /// Node API for querying node state
    node_api: Arc<dyn NodeAPI>,
    /// Monotonic outbound packet sequence (assigned at send).
    outbound_sequence: AtomicU64,
    /// Per-peer ingress rate limiter.
    rate_limiter: Arc<RateLimiter>,
    /// Per-source ingress sequence guard.
    packet_sequences: Arc<PacketSequenceGuard>,
    /// Locally delivered app payloads awaiting external poll (e.g. BitSov UKM ingress).
    local_delivery_queue: Arc<Mutex<Vec<LocalDelivery>>>,
}

impl MeshManager {
    /// Create a new mesh manager
    pub async fn new(
        ctx: &blvm_node::module::traits::ModuleContext,
        node_api: Arc<dyn NodeAPI>,
        _config: Option<&MeshConfig>,
    ) -> Result<Self, MeshError> {
        let enabled = ctx.get_config_or("mesh.enabled", "false") == "true";
        let mode_str = ctx.get_config_or("mesh.mode", "payment_gated");
        let mode = MeshMode::from(mode_str.as_str());

        let routing_policy = RwLock::new(RoutingPolicyEngine::new(mode));
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

        let data_dir = std::path::Path::new(&ctx.data_dir);
        let identity = Arc::new(MeshIdentity::load_or_create(data_dir)?);
        let node_id = identity.node_id();

        let rate_limit = _config.map(|c| c.rate_limit_per_minute).unwrap_or_else(|| {
            ctx.get_config_or("mesh.rate_limit_per_minute", "120")
                .parse::<u32>()
                .unwrap_or(120)
        });
        let rate_limiter = Arc::new(RateLimiter::new(rate_limit, 60));

        debug!(
            "Initializing mesh manager: enabled={}, mode={:?}, node_id={}",
            enabled,
            mode,
            hex::encode(&node_id[..8])
        );

        Ok(Self {
            enabled,
            routing_policy,
            payment_verifier,
            replay_prevention,
            routing_table,
            route_discovery,
            identity,
            node_id,
            node_api,
            outbound_sequence: AtomicU64::new(1),
            rate_limiter,
            packet_sequences: Arc::new(PacketSequenceGuard::new()),
            local_delivery_queue: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Drain locally delivered app packets (optionally filtered by protocol id).
    pub async fn poll_local_deliveries(
        &self,
        protocol_filter: Option<&str>,
        max: usize,
    ) -> Vec<LocalDelivery> {
        let max = max.clamp(1, 64);
        let mut queue = self.local_delivery_queue.lock().await;
        let mut out = Vec::new();
        let mut i = 0;
        while i < queue.len() && out.len() < max {
            if protocol_filter
                .map(|p| queue[i].protocol_id == p)
                .unwrap_or(true)
            {
                out.push(queue.remove(i));
            } else {
                i += 1;
            }
        }
        out
    }

    async fn enqueue_local_delivery(&self, protocol_id: &str, payload: Vec<u8>, source: NodeId) {
        let mut queue = self.local_delivery_queue.lock().await;
        if queue.len() >= LOCAL_DELIVERY_QUEUE_CAP {
            warn!(
                protocol_id,
                cap = LOCAL_DELIVERY_QUEUE_CAP,
                "local delivery queue full — dropping oldest"
            );
            queue.remove(0);
        }
        queue.push(LocalDelivery {
            protocol_id: protocol_id.to_string(),
            payload,
            source,
        });
    }

    /// Assign the next monotonic sequence to an outbound packet from this node.
    pub fn prepare_outbound_packet(&self, packet: &mut MeshPacket) {
        if packet.source == self.node_id {
            packet.sequence = self.outbound_sequence.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Next outbound sequence without mutating a packet (for tests).
    pub fn next_sequence(&self) -> u64 {
        self.outbound_sequence.fetch_add(1, Ordering::SeqCst)
    }

    /// Load static peers from config (G5).
    pub fn load_configured_peers(&self, config: &MeshConfig) -> Result<(), MeshError> {
        for peer in &config.peers {
            let node_id = peer
                .node_id_hex
                .as_ref()
                .map(|h| Self::parse_node_id_hex(h))
                .transpose()?;
            self.add_peer_with_id(peer.address.trim(), node_id)?;
        }
        for addr in &config.peer_list {
            let addr = addr.trim();
            if !addr.is_empty() {
                self.add_peer(addr)?;
            }
        }
        for addr in &config.bootstrap_nodes {
            let addr = addr.trim();
            if !addr.is_empty() {
                self.add_peer(addr)?;
            }
        }
        Ok(())
    }

    /// Determine routing policy for raw wire bytes (payload sniffing / interop).
    pub fn determine_routing_policy(&self, message: &[u8]) -> crate::routing_policy::RoutingPolicy {
        if !self.enabled {
            return crate::routing_policy::RoutingPolicy::Free;
        }

        let policy = self.routing_policy.read().unwrap();
        let protocol = policy.detect_protocol(message);
        policy.determine_policy(protocol)
    }

    /// Determine routing policy for a mesh packet (uses `packet_type`, G7).
    pub fn determine_routing_policy_for_packet(
        &self,
        packet: &MeshPacket,
    ) -> crate::routing_policy::RoutingPolicy {
        if !self.enabled {
            return crate::routing_policy::RoutingPolicy::Free;
        }

        let policy = self.routing_policy.read().unwrap();
        policy.determine_policy_for_packet_type(packet.packet_type.clone())
    }

    /// Start the mesh manager
    pub async fn start(&self) -> Result<(), MeshError> {
        debug!(
            "Starting mesh manager (enabled={}, mode={:?})",
            self.enabled,
            self.routing_policy.read().unwrap().mode()
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
        let mut packet = packet.clone();
        if packet.source == self.node_id && packet.sequence == 0 {
            self.prepare_outbound_packet(&mut packet);
        }

        if !self.enabled {
            return Err(MeshError::MeshDisabled("Mesh is disabled".to_string()));
        }

        // Early exit: Check if packet payload is empty (cheap check before expensive validation)
        if packet.payload.is_empty() {
            return Err(MeshError::InvalidPacket("Empty payload".to_string()));
        }

        // Early exit: Check if destination is valid (cheap check)
        if packet.destination == [0u8; 32] {
            return Err(MeshError::InvalidPacket(
                "Invalid destination (zero hash)".to_string(),
            ));
        }

        // Validate packet structure
        packet.validate().map_err(MeshError::InvalidPacket)?;

        // Determine routing policy from packet type (mesh-originated packets)
        let policy = self.determine_routing_policy_for_packet(&packet);

        // Check if payment is required
        if policy == crate::routing_policy::RoutingPolicy::PaymentRequired {
            // Verify payment proof
            if let Some(ref proof) = packet.payment_proof {
                // Check replay prevention (lock-free with DashMap)
                let replay = self.replay_prevention.lock().await;
                replay
                    .check_replay(proof, &packet.source, packet.sequence)
                    .map_err(MeshError::ReplayDetected)?;

                // Verify payment
                let verification = self
                    .payment_verifier
                    .verify(proof)
                    .await
                    .map_err(|e| MeshError::PaymentVerification(e.to_string()))?;

                if !verification.verified {
                    return Err(MeshError::PaymentVerification(
                        verification
                            .error
                            .unwrap_or_else(|| "Payment verification failed".to_string()),
                    ));
                }

                debug!(
                    "Payment verified: amount={} sats, destination={:x?}",
                    verification.amount,
                    &packet.destination[..8]
                );
            } else {
                return Err(MeshError::PaymentVerification(
                    "Payment proof required for paid packets".to_string(),
                ));
            }
        }

        // Route the packet
        self.forward_packet(&packet).await?;

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
            return Err(MeshError::InvalidPacket(
                "Invalid destination (zero hash)".to_string(),
            ));
        }

        // Use cached node ID (computed at init)
        let _my_node_id = self.node_id;

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
                        entry
                            .direct_address
                            .as_ref()
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
                    entry
                        .direct_address
                        .as_ref()
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
    async fn send_mesh_packet(
        &self,
        peer_address: String,
        packet_data: Vec<u8>,
    ) -> Result<(), MeshError> {
        // Send packet via NodeAPI to network layer
        self.node_api
            .send_mesh_packet_to_peer(peer_address, packet_data)
            .await
            .map_err(|e| MeshError::NetworkError(format!("Failed to send mesh packet: {e}")))?;

        debug!("Mesh packet sent successfully");
        Ok(())
    }

    /// Handle raw mesh packet bytes from `MeshPacketReceived` event.
    pub async fn handle_mesh_packet_received(
        &self,
        packet_data: &[u8],
        peer_addr: &str,
    ) -> Result<(), MeshError> {
        if packet_data.len() > crate::packet::MAX_BINCODE_PAYLOAD_SIZE {
            return Err(MeshError::InvalidPacket(format!(
                "Packet too large: {} bytes",
                packet_data.len()
            )));
        }
        let packet = deserialize_mesh_packet(packet_data)?;
        self.handle_incoming_packet(&packet, Some(peer_addr)).await
    }

    /// Handle an incoming mesh packet
    pub async fn handle_incoming_packet(
        &self,
        packet: &MeshPacket,
        peer_addr: Option<&str>,
    ) -> Result<(), MeshError> {
        if !self.enabled {
            return Err(MeshError::MeshDisabled("Mesh is disabled".to_string()));
        }

        packet.validate().map_err(MeshError::InvalidPacket)?;

        let rate_key = peer_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| hex::encode(&packet.source[..8]));
        self.rate_limiter
            .check_and_record(&rate_key)
            .map_err(MeshError::RateLimited)?;

        if let Some(protocol) = packet.metadata.as_ref().and_then(|m| m.protocol.as_deref()) {
            match protocol {
                PROTOCOL_HELLO => {
                    return self
                        .handle_hello_packet(packet, peer_addr.unwrap_or(""))
                        .await;
                }
                PROTOCOL_DISCOVERY => {
                    return self.handle_discovery_packet(packet).await;
                }
                _ => {}
            }
        }

        self.packet_sequences
            .check_and_record(&packet.source, packet.sequence)
            .map_err(MeshError::ReplayDetected)?;

        if !self.verify_forward_payment(packet).await? {
            return Err(MeshError::PaymentVerification(
                "Payment proof required for paid forward".to_string(),
            ));
        }

        // Check if packet is for this node
        if packet.is_for_me(&self.node_id) {
            if let Some(protocol) = packet.metadata.as_ref().and_then(|m| m.protocol.as_deref()) {
                if protocol != PROTOCOL_HELLO && protocol != PROTOCOL_DISCOVERY {
                    self.enqueue_local_delivery(protocol, packet.payload.clone(), packet.source)
                        .await;
                }
            }
            debug!(
                "Packet delivered to local node: source={:x?}",
                &packet.source[..8]
            );
            debug!(
                "Delivered mesh packet to destination: {:x?}",
                &packet.destination[..8]
            );
            return Ok(());
        }

        // Forward if we're on the packet route or have a table route to the destination.
        let can_reach = packet.should_forward(&self.node_id)
            || self.routing_table.find_route(&packet.destination).is_some()
            || self.routing_table.get_route(&packet.destination).is_some();

        if can_reach {
            debug!(
                "Forwarding packet: destination={:x?}",
                &packet.destination[..8]
            );
            self.forward_packet(packet).await?;
        } else {
            warn!("Dropping packet: not for us and no route to destination");
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
                        if let EventPayload::PeerConnected {
                            peer_addr,
                            transport_type,
                            peer_node_id,
                            ..
                        } = &event_msg.payload
                        {
                            if let Some(bytes) = peer_node_id.as_ref().filter(|b| b.len() == 32) {
                                let mut id = [0u8; 32];
                                id.copy_from_slice(bytes);
                                self.routing_table
                                    .add_direct_peer(id, peer_addr.as_bytes().to_vec());
                                info!(
                                    "Peer identity updated: addr={}, node_id={:x?}",
                                    peer_addr,
                                    &id[..8]
                                );
                                return Ok(());
                            }

                            let derived = Self::derive_node_id_from_address(peer_addr);
                            self.routing_table
                                .add_direct_peer(derived, peer_addr.as_bytes().to_vec());

                            info!(
                                "Peer connected: addr={}, transport={}",
                                peer_addr, transport_type
                            );

                            if let Err(e) = self.send_hello_to_peer(peer_addr).await {
                                warn!("Failed to send mesh hello to {}: {}", peer_addr, e);
                            }
                        }
                    }
                    EventType::PeerDisconnected => {
                        debug!("Peer disconnected event received");
                        if let EventPayload::PeerDisconnected { peer_addr, .. } = &event_msg.payload
                        {
                            let peer_node_id = self
                                .routing_table
                                .node_id_for_address(peer_addr)
                                .unwrap_or_else(|| Self::derive_node_id_from_address(peer_addr));

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

    /// List routing table entries
    pub fn list_routes(&self) -> Vec<(NodeId, bool, usize, u64)> {
        self.routing_table.list_routes()
    }

    /// List direct peers (node_id, address)
    pub fn list_direct_peers(&self) -> Vec<(NodeId, String)> {
        self.routing_table.list_direct_peers()
    }

    /// Parse a 64-char hex NodeId.
    pub fn parse_node_id_hex(hex_str: &str) -> Result<NodeId, MeshError> {
        let hex_str = hex_str.trim().trim_start_matches("0x");
        if hex_str.len() != 64 || !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(MeshError::InvalidPacket(format!(
                "NodeId must be 64 hex chars, got: {hex_str}"
            )));
        }
        let bytes = hex::decode(hex_str)
            .map_err(|e| MeshError::InvalidPacket(format!("Invalid node_id hex: {e}")))?;
        let mut node_id = [0u8; 32];
        node_id.copy_from_slice(&bytes);
        Ok(node_id)
    }

    /// Add a direct peer; optional explicit node id (G13 / -1.7).
    pub fn add_peer_with_id(&self, addr: &str, node_id: Option<NodeId>) -> Result<(), MeshError> {
        let node_id = node_id.unwrap_or_else(|| Self::derive_node_id_from_address(addr));
        let address_bytes = addr.as_bytes().to_vec();
        self.routing_table.add_direct_peer(node_id, address_bytes);
        info!("Added peer: addr={}, node_id={:x?}", addr, &node_id[..8]);
        Ok(())
    }

    /// Add a direct peer by address (node_id derived from address)
    pub fn add_peer(&self, addr: &str) -> Result<(), MeshError> {
        self.add_peer_with_id(addr, None)
    }

    /// Remove a direct peer by address (node_id derived from address)
    pub fn remove_peer(&self, addr: &str) -> Result<(), MeshError> {
        let node_id = Self::derive_node_id_from_address(addr);
        self.routing_table.remove_direct_peer(&node_id);
        info!(
            "Removed peer via CLI: addr={}, node_id={:x?}",
            addr,
            &node_id[..8]
        );
        Ok(())
    }

    /// Set mesh operating mode at runtime
    pub fn set_mode(&self, mode: MeshMode) -> Result<(), MeshError> {
        let mut policy = self.routing_policy.write().unwrap();
        policy.set_mode(mode);
        info!("Mesh mode set to {:?}", mode);
        Ok(())
    }

    /// Get routing statistics
    pub async fn get_stats(&self) -> MeshStats {
        let routing_stats = self.routing_table.stats();
        let replay_stats = self.replay_prevention.lock().await.stats();

        MeshStats {
            enabled: self.enabled,
            mode: self.routing_policy.read().unwrap().mode(),
            routing: routing_stats,
            replay: replay_stats,
        }
    }

    /// Get node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Estimate routing fee in satoshis for a destination (direct or cached route).
    pub fn quote_route_fee_sats(&self, destination: NodeId, base_fee_sats: u64) -> u64 {
        if let Some(route) = self.routing_table.find_route(&destination) {
            return self
                .routing_table
                .calculate_routing_fee(&route, base_fee_sats)
                .total;
        }
        if self.routing_table.get_route(&destination).is_some() {
            return base_fee_sats;
        }
        0
    }

    /// Issue a Lightning hop invoice via the co-located blvm-lightning module.
    ///
    /// Exposed to BitSov via the module RPC extender as `meshrequesthopinvoice`.
    pub async fn request_hop_invoice(
        &self,
        destination: NodeId,
        amount_msats: u64,
        expiry_seconds: u64,
    ) -> Result<crate::api::HopInvoiceResponse, MeshError> {
        if !self.enabled {
            return Err(MeshError::MeshDisabled("Mesh is disabled".to_string()));
        }
        if amount_msats == 0 {
            return Err(MeshError::InvalidPacket(
                "amount_msats must be greater than zero".to_string(),
            ));
        }

        const LIGHTNING_MODULE_ID: &str = "blvm-lightning";
        let description = format!("blvm-mesh hop to {}", hex::encode(&destination[..8]));
        let params = serde_json::json!({
            "amount_msats": amount_msats,
            "description": description,
            "expiry_seconds": expiry_seconds,
        });
        let params_bytes = serde_json::to_vec(&params).map_err(|e| {
            MeshError::PaymentError(format!("hop invoice params encode failed: {e}"))
        })?;

        let response_bytes = self
            .node_api
            .call_module(Some(LIGHTNING_MODULE_ID), "create_invoice", params_bytes)
            .await
            .map_err(|e| {
                MeshError::PaymentError(format!("lightning create_invoice failed: {e}"))
            })?;

        let response: serde_json::Value = serde_json::from_slice(&response_bytes).map_err(|e| {
            MeshError::PaymentError(format!("hop invoice response decode failed: {e}"))
        })?;
        let invoice = response
            .get("invoice")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                MeshError::PaymentError("create_invoice response missing invoice".to_string())
            })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(crate::api::HopInvoiceResponse {
            invoice: invoice.to_string(),
            amount_msats,
            expires_at: now + expiry_seconds,
        })
    }

    /// Test helper: install a multi-hop route in the local table.
    pub fn install_route_for_test(&self, destination: NodeId, route_path: Vec<NodeId>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let next_hop = route_path.get(1).copied();
        let cost = route_path.len() as u64 * 10;
        self.routing_table.add_route(RoutingEntry {
            node_id: destination,
            direct_address: None,
            next_hop,
            route_path,
            route_cost: cost,
            last_updated: now,
            quality_score: 0.9,
        });
    }

    /// Discover route to destination (network discovery when not cached).
    pub async fn discover_route(
        &self,
        destination: NodeId,
    ) -> Result<Option<Vec<NodeId>>, MeshError> {
        match self
            .route_discovery
            .begin_discovery(destination, self.node_id)
            .await?
        {
            DiscoveryStart::Found(route) => {
                self.publish_route_discovered(&destination, &route).await;
                Ok(Some(route))
            }
            DiscoveryStart::Pending {
                request_id,
                message,
            } => {
                let rx = self.route_discovery.register_waiter(request_id).await;
                self.broadcast_discovery(&message).await?;
                let timeout =
                    std::time::Duration::from_secs(self.route_discovery.timeout_seconds());
                match tokio::time::timeout(timeout, rx).await {
                    Ok(Ok(route)) => {
                        self.publish_route_discovered(&destination, &route).await;
                        Ok(Some(route))
                    }
                    Ok(Err(_)) => {
                        self.publish_route_failed(&destination, "discovery cancelled")
                            .await;
                        Ok(None)
                    }
                    Err(_) => {
                        self.publish_route_failed(&destination, "discovery timeout")
                            .await;
                        Ok(None)
                    }
                }
            }
        }
    }

    async fn send_hello_to_peer(&self, peer_addr: &str) -> Result<(), MeshError> {
        let temp_id = Self::derive_node_id_from_address(peer_addr);
        let mut packet = MeshPacket::new(
            PacketType::Paid,
            self.node_id,
            temp_id,
            self.identity.hello_payload(),
        );
        packet.metadata = Some(PacketMetadata {
            protocol: Some(PROTOCOL_HELLO.to_string()),
            fields: std::collections::HashMap::new(),
        });
        let wire = serialize_mesh_packet(&packet)?;
        self.send_mesh_packet(peer_addr.to_string(), wire).await
    }

    async fn handle_hello_packet(
        &self,
        packet: &MeshPacket,
        peer_addr: &str,
    ) -> Result<(), MeshError> {
        let peer_id = MeshIdentity::parse_hello_payload(&packet.payload)?;
        let stale_id = Self::derive_node_id_from_address(peer_addr);
        if stale_id != peer_id {
            self.routing_table.remove_direct_peer(&stale_id);
        }
        self.add_peer_with_id(peer_addr, Some(peer_id))?;
        info!(
            "Mesh hello from {}: node_id={}",
            peer_addr,
            hex::encode(&peer_id[..8])
        );
        let node_api = Arc::clone(&self.node_api);
        let peer_addr_owned = peer_addr.to_string();
        tokio::spawn(async move {
            let _ = node_api
                .publish_event(
                    EventType::PeerConnected,
                    EventPayload::PeerConnected {
                        peer_addr: peer_addr_owned,
                        transport_type: "mesh-hello".to_string(),
                        services: 0,
                        version: 0,
                        peer_node_id: Some(peer_id.to_vec()),
                    },
                )
                .await;
        });
        Ok(())
    }

    async fn handle_discovery_packet(&self, packet: &MeshPacket) -> Result<(), MeshError> {
        let message: DiscoveryMessage = bincode::deserialize(&packet.payload)
            .map_err(|e| MeshError::InvalidPacket(format!("discovery decode: {e}")))?;

        match &message {
            DiscoveryMessage::RouteRequest { .. } => {
                if let Some(response) = self
                    .route_discovery
                    .handle_route_request(&message, self.node_id)
                    .await?
                {
                    self.send_discovery_to(packet.source, response).await?;
                }
            }
            DiscoveryMessage::RouteResponse { .. } => {
                self.route_discovery
                    .handle_route_response(&message, packet.source)
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn send_discovery_to(
        &self,
        dest: NodeId,
        message: DiscoveryMessage,
    ) -> Result<(), MeshError> {
        let payload = bincode::serialize(&message)
            .map_err(|e| MeshError::InvalidPacket(format!("discovery encode: {e}")))?;
        self.send_control_packet(dest, PROTOCOL_DISCOVERY, payload)
            .await
    }

    async fn broadcast_discovery(&self, message: &DiscoveryMessage) -> Result<(), MeshError> {
        let peers: Vec<NodeId> = self
            .list_direct_peers()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        for peer in peers {
            if let Err(e) = self.send_discovery_to(peer, message.clone()).await {
                warn!("Discovery broadcast to {:x?} failed: {}", &peer[..8], e);
            }
        }
        Ok(())
    }

    async fn send_control_packet(
        &self,
        dest: NodeId,
        protocol: &str,
        payload: Vec<u8>,
    ) -> Result<(), MeshError> {
        let mut packet = MeshPacket::new(PacketType::Paid, self.node_id, dest, payload);
        packet.metadata = Some(PacketMetadata {
            protocol: Some(protocol.to_string()),
            fields: std::collections::HashMap::new(),
        });
        self.route_packet(&packet).await
    }

    async fn verify_forward_payment(&self, packet: &MeshPacket) -> Result<bool, MeshError> {
        let policy = self.determine_routing_policy_for_packet(packet);
        if policy != crate::routing_policy::RoutingPolicy::PaymentRequired {
            return Ok(true);
        }
        if let Some(ref proof) = packet.payment_proof {
            let replay = self.replay_prevention.lock().await;
            replay
                .check_replay(proof, &packet.source, packet.sequence)
                .map_err(MeshError::ReplayDetected)?;
            let verification = self.payment_verifier.verify(proof).await?;
            return Ok(verification.verified);
        }
        Ok(false)
    }

    async fn publish_route_discovered(&self, destination: &NodeId, route: &[NodeId]) {
        let path: Vec<String> = self
            .list_direct_peers()
            .into_iter()
            .map(|(_, addr)| addr)
            .take(route.len())
            .collect();
        let _ = self
            .node_api
            .publish_event(
                EventType::RouteDiscovered,
                EventPayload::RouteDiscovered {
                    destination: destination.to_vec(),
                    route_path: if path.is_empty() {
                        vec![hex::encode(&route[0][..8])]
                    } else {
                        path
                    },
                    route_cost: route.len() as u64 * 10,
                },
            )
            .await;
    }

    async fn publish_route_failed(&self, destination: &NodeId, reason: &str) {
        let _ = self
            .node_api
            .publish_event(
                EventType::RouteFailed,
                EventPayload::RouteFailed {
                    destination: destination.to_vec(),
                    reason: reason.to_string(),
                },
            )
            .await;
    }

    /// Test constructor with explicit node id (no module DB).
    pub async fn new_for_test(
        enabled: bool,
        mode: MeshMode,
        node_id: NodeId,
        node_api: Arc<dyn NodeAPI>,
    ) -> Self {
        Self::new_for_test_with_identity(
            enabled,
            mode,
            Arc::new(MeshIdentity::from_seed(node_id[0])),
            Some(node_id),
            node_api,
        )
        .await
    }

    /// Test constructor using Ed25519 identity (D-1 aligned).
    pub async fn new_for_test_with_seed(
        enabled: bool,
        mode: MeshMode,
        seed: u8,
        node_api: Arc<dyn NodeAPI>,
    ) -> Self {
        Self::new_for_test_with_identity(
            enabled,
            mode,
            Arc::new(MeshIdentity::from_seed(seed)),
            None,
            node_api,
        )
        .await
    }

    async fn new_for_test_with_identity(
        enabled: bool,
        mode: MeshMode,
        identity: Arc<MeshIdentity>,
        node_id_override: Option<NodeId>,
        node_api: Arc<dyn NodeAPI>,
    ) -> Self {
        Self::new_for_test_with_identity_and_rate_limit(
            enabled,
            mode,
            identity,
            node_id_override,
            node_api,
            0,
        )
        .await
    }

    async fn new_for_test_with_identity_and_rate_limit(
        enabled: bool,
        mode: MeshMode,
        identity: Arc<MeshIdentity>,
        node_id_override: Option<NodeId>,
        node_api: Arc<dyn NodeAPI>,
        rate_limit_per_minute: u32,
    ) -> Self {
        let routing_policy = RwLock::new(RoutingPolicyEngine::new(mode));
        let payment_verifier = PaymentVerifier::new(Arc::clone(&node_api));
        const REPLAY_EXPIRY_SECONDS: u64 = 24 * 60 * 60;
        let replay_prevention = Arc::new(Mutex::new(ReplayPrevention::new(REPLAY_EXPIRY_SECONDS)));
        const ROUTE_EXPIRY_SECONDS: u64 = 60 * 60;
        let routing_table = Arc::new(RoutingTable::new(ROUTE_EXPIRY_SECONDS));
        let route_discovery = Arc::new(RouteDiscovery::new(Arc::clone(&routing_table), 10, 30));
        let node_id = node_id_override.unwrap_or_else(|| identity.node_id());

        Self {
            enabled,
            routing_policy,
            payment_verifier,
            replay_prevention,
            routing_table,
            route_discovery,
            identity,
            node_id,
            node_api,
            outbound_sequence: AtomicU64::new(1),
            rate_limiter: Arc::new(RateLimiter::new(rate_limit_per_minute, 60)),
            packet_sequences: Arc::new(PacketSequenceGuard::new()),
            local_delivery_queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Test helper: manager with a strict ingress rate limit.
    pub async fn new_for_test_with_rate_limit(
        enabled: bool,
        mode: MeshMode,
        seed: u8,
        node_api: Arc<dyn NodeAPI>,
        rate_limit_per_minute: u32,
    ) -> Self {
        Self::new_for_test_with_identity_and_rate_limit(
            enabled,
            mode,
            Arc::new(MeshIdentity::from_seed(seed)),
            None,
            node_api,
            rate_limit_per_minute,
        )
        .await
    }

    /// Derive node ID from peer address (simplified - in production would use peer's public key)
    pub(crate) fn derive_node_id_from_address(peer_addr: &str) -> NodeId {
        // In production, this would:
        // 1. Get peer's public key from handshake or peer info
        // 2. SHA256 hash the public key
        // 3. Use first 32 bytes as node ID

        // For now, derive from address (not secure, but functional for testing)
        let mut hasher = Sha256::new();
        hasher.update(peer_addr.as_bytes());
        let hash = hasher.finalize();
        let mut node_id = [0u8; 32];
        node_id.copy_from_slice(&hash);
        node_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{MeshPacket, PacketType};
    use crate::routing_policy::{MeshMode, RoutingPolicy};
    use crate::test_support::TestNodeAPI;
    use std::sync::Arc;

    fn id(byte: u8) -> NodeId {
        [byte; 32]
    }

    #[tokio::test]
    async fn add_peer_with_explicit_node_id_stores_route() {
        let manager = MeshManager::new_for_test(
            true,
            MeshMode::Open,
            id(1),
            Arc::new(TestNodeAPI::default()),
        )
        .await;
        let peer_id = id(2);
        manager
            .add_peer_with_id("127.0.0.1:18333", Some(peer_id))
            .unwrap();
        let peers = manager.list_direct_peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].0, peer_id);
        assert_eq!(peers[0].1, "127.0.0.1:18333");
    }

    #[tokio::test]
    async fn add_peer_without_node_id_derives_from_address() {
        let manager = MeshManager::new_for_test(
            true,
            MeshMode::Open,
            id(1),
            Arc::new(TestNodeAPI::default()),
        )
        .await;
        manager.add_peer("10.0.0.1:8333").unwrap();
        let expected = MeshManager::derive_node_id_from_address("10.0.0.1:8333");
        let peers = manager.list_direct_peers();
        assert_eq!(peers[0].0, expected);
    }

    #[tokio::test]
    async fn handle_incoming_packet_delivers_when_destination_matches() {
        let local = id(42);
        let manager = MeshManager::new_for_test(
            true,
            MeshMode::Open,
            local,
            Arc::new(TestNodeAPI::default()),
        )
        .await;
        let packet = MeshPacket::new(PacketType::Paid, id(1), local, b"hello".to_vec());
        manager.handle_incoming_packet(&packet, None).await.unwrap();
    }

    #[tokio::test]
    async fn local_delivery_enqueued_for_app_protocol() {
        let local = id(42);
        let manager = MeshManager::new_for_test(
            true,
            MeshMode::Open,
            local,
            Arc::new(TestNodeAPI::default()),
        )
        .await;
        let mut packet = MeshPacket::new(PacketType::Paid, id(1), local, b"ukm-json".to_vec());
        packet.metadata = Some(PacketMetadata {
            protocol: Some("bitsov-ukm-v1".to_string()),
            fields: Default::default(),
        });
        manager.handle_incoming_packet(&packet, None).await.unwrap();
        let deliveries = manager
            .poll_local_deliveries(Some("bitsov-ukm-v1"), 8)
            .await;
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].payload, b"ukm-json");
        assert!(manager
            .poll_local_deliveries(Some("bitsov-ukm-v1"), 8)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn handle_mesh_packet_received_deserializes_and_delivers() {
        let local = id(7);
        let manager = MeshManager::new_for_test(
            true,
            MeshMode::Open,
            local,
            Arc::new(TestNodeAPI::default()),
        )
        .await;
        let packet = MeshPacket::new(PacketType::Paid, id(1), local, b"wire".to_vec());
        let wire = serialize_mesh_packet(&packet).unwrap();
        manager
            .handle_mesh_packet_received(&wire, "127.0.0.1:1")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn route_packet_open_mode_accepts_paid_without_proof() {
        let manager = MeshManager::new_for_test(
            true,
            MeshMode::Open,
            id(1),
            Arc::new(TestNodeAPI::default()),
        )
        .await;
        manager
            .add_peer_with_id("127.0.0.1:2", Some(id(2)))
            .unwrap();
        let packet = MeshPacket::new(PacketType::Paid, id(1), id(2), b"payload".to_vec());
        manager.route_packet(&packet).await.unwrap();
    }

    #[tokio::test]
    async fn route_packet_payment_gated_rejects_without_proof() {
        let manager = MeshManager::new_for_test(
            true,
            MeshMode::PaymentGated,
            id(1),
            Arc::new(TestNodeAPI::default()),
        )
        .await;
        manager
            .add_peer_with_id("127.0.0.1:2", Some(id(2)))
            .unwrap();
        let packet = MeshPacket::new(PacketType::Paid, id(1), id(2), b"payload".to_vec());
        let err = manager.route_packet(&packet).await.unwrap_err();
        assert!(matches!(err, MeshError::PaymentVerification(_)));
    }

    #[test]
    fn determine_routing_policy_for_packet_uses_type_not_payload() {
        let manager = tokio::runtime::Runtime::new().unwrap().block_on(async {
            MeshManager::new_for_test(
                true,
                MeshMode::PaymentGated,
                id(1),
                Arc::new(TestNodeAPI::default()),
            )
            .await
        });
        // Opaque app bytes would sniff as Unknown; Paid type should map to MeshPacket policy.
        let packet = MeshPacket::new(PacketType::Paid, id(1), id(2), vec![0xde, 0xad]);
        let policy = manager.determine_routing_policy_for_packet(&packet);
        assert_eq!(policy, RoutingPolicy::PaymentRequired);
        // Same payload bytes sniffed directly would also be Unknown → payment in payment_gated
        let sniffed = manager.determine_routing_policy(&[0xde, 0xad]);
        assert_eq!(sniffed, RoutingPolicy::PaymentRequired);
    }
}
