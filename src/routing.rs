//! Mesh routing table management
//!
//! Manages routing table for mesh networking, including route discovery,
//! fee calculation, and multi-hop routing.

use crate::error::MeshError;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Node ID (32 bytes, SHA256 of public key)
pub type NodeId = [u8; 32];

/// Routing entry for a mesh node
#[derive(Debug, Clone)]
pub struct RoutingEntry {
    /// Node ID
    pub node_id: NodeId,
    /// Direct peer address (if directly connected)
    pub direct_address: Option<Vec<u8>>, // Could be SocketAddr or Iroh NodeId
    /// Next hop node ID (if multi-hop)
    pub next_hop: Option<NodeId>,
    /// Route path (list of node IDs to reach destination)
    pub route_path: Vec<NodeId>,
    /// Route cost (in satoshis, for fee calculation)
    pub route_cost: u64,
    /// Last updated timestamp
    pub last_updated: u64,
    /// Route quality score (0.0 to 1.0)
    pub quality_score: f64,
}

/// Routing table for mesh networking
///
/// Uses DashMap for lock-free concurrent access, providing better performance
/// than RwLock<HashMap> for read-heavy workloads.
pub struct RoutingTable {
    /// Routing entries (node_id -> RoutingEntry)
    /// Lock-free concurrent reads, no async needed
    routes: Arc<DashMap<NodeId, RoutingEntry>>,
    /// Direct peers (node_id -> address)
    /// Lock-free concurrent reads, no async needed
    direct_peers: Arc<DashMap<NodeId, Vec<u8>>>,
    /// Route discovery cache (destination -> route)
    /// Lock-free concurrent reads, no async needed
    route_cache: Arc<DashMap<NodeId, Vec<NodeId>>>,
    /// Route expiry time (default: 1 hour)
    route_expiry_seconds: u64,
}

impl RoutingTable {
    /// Create a new routing table
    pub fn new(route_expiry_seconds: u64) -> Self {
        Self {
            routes: Arc::new(DashMap::new()),
            direct_peers: Arc::new(DashMap::new()),
            route_cache: Arc::new(DashMap::new()),
            route_expiry_seconds,
        }
    }

    /// Add or update a direct peer
    ///
    /// Lock-free operation using DashMap - no async needed
    pub fn add_direct_peer(&self, node_id: NodeId, address: Vec<u8>) {
        // Lock-free insert
        self.direct_peers.insert(node_id, address.clone());
        
        // Update routing entry (lock-free)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        self.routes.insert(
            node_id,
            RoutingEntry {
                node_id,
                direct_address: Some(address),
                next_hop: None, // Direct connection
                route_path: vec![node_id],
                route_cost: 0, // Direct connections have no routing cost
                last_updated: now,
                quality_score: 1.0, // Direct connections have perfect quality
            },
        );
        
        debug!("Added direct peer: node_id={:x?}", &node_id[..8]);
    }

    /// Remove a direct peer
    ///
    /// Lock-free operation using DashMap - no async needed
    pub fn remove_direct_peer(&self, node_id: &NodeId) {
        // Lock-free remove
        self.direct_peers.remove(node_id);
        
        // Remove routing entry if it was direct-only (lock-free)
        if let Some(entry) = self.routes.get(node_id) {
            if entry.direct_address.is_some() && entry.next_hop.is_none() {
                self.routes.remove(node_id);
                debug!("Removed direct peer: node_id={:x?}", &node_id[..8]);
            }
        }
    }

    /// Add or update a routing entry
    ///
    /// Lock-free operation using DashMap - no async needed
    pub fn add_route(&self, entry: RoutingEntry) {
        // Lock-free insert
        self.routes.insert(entry.node_id, entry.clone());
        debug!("Added route: node_id={:x?}", &entry.node_id[..8]);
    }

    /// Get routing entry for a node
    ///
    /// Lock-free read using DashMap - no async needed
    pub fn get_route(&self, node_id: &NodeId) -> Option<RoutingEntry> {
        // Lock-free get
        self.routes.get(node_id).map(|entry| entry.value().clone())
    }

    /// Find route to destination (with route discovery if needed)
    ///
    /// Lock-free reads using DashMap - no async needed
    pub fn find_route(&self, destination: &NodeId) -> Option<Vec<NodeId>> {
        // Check cache first (lock-free)
        if let Some(route) = self.route_cache.get(destination) {
            return Some(route.value().clone());
        }

        // Check direct routes (lock-free)
        if let Some(entry) = self.routes.get(destination) {
            // Check if route is still valid (not expired)
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            if now <= entry.last_updated + self.route_expiry_seconds {
                let route = entry.route_path.clone();
                
                // Cache the route (lock-free insert)
                self.route_cache.insert(*destination, route.clone());
                
                return Some(route);
            }
        }

        // Route not found - would need route discovery
        // For now, return None (route discovery to be implemented)
        None
    }

    /// Calculate routing fee for a route
    ///
    /// Fee calculation: 60% to destination, 30% to intermediate nodes, 10% to source
    pub fn calculate_routing_fee(&self, route: &[NodeId], base_fee_sats: u64) -> RoutingFee {
        let total_fee = base_fee_sats;
        
        // Split: 60% destination, 30% intermediate, 10% source
        let destination_fee = (total_fee * 60) / 100;
        let intermediate_fee = if route.len() > 2 {
            (total_fee * 30) / 100 / (route.len() - 2) as u64
        } else {
            0
        };
        let source_fee = (total_fee * 10) / 100;

        RoutingFee {
            total: total_fee,
            destination: destination_fee,
            intermediate: intermediate_fee,
            source: source_fee,
            hop_count: route.len(),
        }
    }

    /// Clean up expired routes
    ///
    /// Lock-free operations using DashMap - no async needed
    pub fn cleanup_expired(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut expired = Vec::new();

        // Lock-free iteration
        for entry in self.routes.iter() {
            if now > entry.value().last_updated + self.route_expiry_seconds {
                // Don't expire direct peers
                if entry.value().direct_address.is_none() || entry.value().next_hop.is_some() {
                    expired.push(*entry.key());
                }
            }
        }

        // Lock-free removal
        for node_id in &expired {
            self.routes.remove(node_id);
        }

        if !expired.is_empty() {
            debug!("Cleaned up {} expired routes", expired.len());
        }

        // Clean up route cache (lock-free clear)
        self.route_cache.clear(); // Clear cache on cleanup (will be repopulated as needed)
    }

    /// Get routing statistics
    ///
    /// Lock-free reads using DashMap - no async needed
    pub fn stats(&self) -> RoutingStats {
        RoutingStats {
            total_routes: self.routes.len(),
            direct_peers: self.direct_peers.len(),
            cached_routes: self.route_cache.len(),
            route_expiry_seconds: self.route_expiry_seconds,
        }
    }
}

/// Routing fee breakdown
#[derive(Debug, Clone)]
pub struct RoutingFee {
    /// Total fee in satoshis
    pub total: u64,
    /// Fee to destination (60%)
    pub destination: u64,
    /// Fee per intermediate node (30% split)
    pub intermediate: u64,
    /// Fee to source node (10%)
    pub source: u64,
    /// Number of hops
    pub hop_count: usize,
}

/// Routing statistics
#[derive(Debug, Clone)]
pub struct RoutingStats {
    /// Total number of routes
    pub total_routes: usize,
    /// Number of direct peers
    pub direct_peers: usize,
    /// Number of cached routes
    pub cached_routes: usize,
    /// Route expiry time in seconds
    pub route_expiry_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_direct_peer() {
        let table = RoutingTable::new(3600);
        let node_id = [1u8; 32];
        let address = vec![127, 0, 0, 1, 0, 80]; // Example address

        table.add_direct_peer(node_id, address);

        let route = table.get_route(&node_id);
        assert!(route.is_some());
        let route = route.unwrap();
        assert_eq!(route.node_id, node_id);
        assert!(route.direct_address.is_some());
        assert_eq!(route.route_path, vec![node_id]);
    }

    #[tokio::test]
    async fn test_route_discovery() {
        let table = RoutingTable::new(3600);
        let destination = [2u8; 32];

        // Route not found (no route discovery yet)
        let route = table.find_route(&destination);
        assert!(route.is_none());
    }

    #[tokio::test]
    async fn test_fee_calculation() {
        let table = RoutingTable::new(3600);
        let route = vec![[1u8; 32], [2u8; 32], [3u8; 32]]; // 3-hop route
        let base_fee = 1000; // 1000 sats

        let fee = table.calculate_routing_fee(&route, base_fee);
        assert_eq!(fee.total, 1000);
        assert_eq!(fee.destination, 600); // 60%
        assert_eq!(fee.intermediate, 300); // 30% / 1 intermediate
        assert_eq!(fee.source, 100); // 10%
        assert_eq!(fee.hop_count, 3);
    }
}
