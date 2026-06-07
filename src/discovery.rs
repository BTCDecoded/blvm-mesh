//! Route discovery protocol for mesh networking
//!
//! Implements route discovery using distance vector routing (simple, scalable later).

use crate::error::MeshError;
use crate::routing::{NodeId, RoutingEntry, RoutingTable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, RwLock};
use tracing::{debug, info};

/// Outcome of starting route discovery.
pub enum DiscoveryStart {
    /// Route already in local table.
    Found(Vec<NodeId>),
    /// Broadcast required; wait on receiver for `request_id`.
    Pending {
        request_id: u64,
        message: DiscoveryMessage,
    },
}

/// Route discovery message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryMessage {
    /// Route request (find route to destination)
    RouteRequest {
        destination: NodeId,
        source: NodeId,
        request_id: u64,
        max_hops: u8,
    },
    /// Route response (route found)
    RouteResponse {
        destination: NodeId,
        source: NodeId,
        request_id: u64,
        route: Vec<NodeId>,
        cost: u64,
    },
    /// Route advertisement (announce routes to neighbors)
    RouteAdvertisement {
        routes: Vec<RouteAdvertisementEntry>,
        source: NodeId,
    },
}

/// Route advertisement entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteAdvertisementEntry {
    pub destination: NodeId,
    pub next_hop: NodeId,
    pub cost: u64,
    pub hop_count: u8,
}

/// Route discovery manager
pub struct RouteDiscovery {
    /// Pending route requests (request_id -> RouteRequest)
    pending_requests: Arc<RwLock<HashMap<u64, PendingRequest>>>,
    /// Waiters for in-flight discovery (request_id -> oneshot)
    waiters: Arc<RwLock<HashMap<u64, oneshot::Sender<Vec<NodeId>>>>>,
    /// Request ID counter
    request_id_counter: Arc<RwLock<u64>>,
    /// Routing table reference
    routing_table: Arc<RoutingTable>,
    /// Maximum route discovery hops
    max_hops: u8,
    /// Route discovery timeout (seconds)
    timeout_seconds: u64,
}

/// Pending route request
#[allow(dead_code)]
struct PendingRequest {
    destination: NodeId,
    source: NodeId,
    request_id: u64,
    timestamp: u64,
    responders: Vec<NodeId>,
}

impl RouteDiscovery {
    /// Create a new route discovery manager
    pub fn new(routing_table: Arc<RoutingTable>, max_hops: u8, timeout_seconds: u64) -> Self {
        Self {
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            waiters: Arc::new(RwLock::new(HashMap::new())),
            request_id_counter: Arc::new(RwLock::new(0)),
            routing_table,
            max_hops,
            timeout_seconds,
        }
    }

    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }

    /// Register a waiter before broadcasting a pending discovery request.
    pub async fn register_waiter(&self, request_id: u64) -> oneshot::Receiver<Vec<NodeId>> {
        let (tx, rx) = oneshot::channel();
        self.waiters.write().await.insert(request_id, tx);
        rx
    }

    /// Generate a new request ID
    async fn next_request_id(&self) -> u64 {
        let mut counter = self.request_id_counter.write().await;
        *counter += 1;
        *counter
    }

    /// Begin route discovery — returns immediately-found route or a pending request.
    pub async fn begin_discovery(
        &self,
        destination: NodeId,
        source: NodeId,
    ) -> Result<DiscoveryStart, MeshError> {
        if let Some(route) = self.routing_table.find_route(&destination) {
            return Ok(DiscoveryStart::Found(route));
        }

        if let Some(entry) = self.routing_table.get_route(&destination) {
            if entry.direct_address.is_some() {
                return Ok(DiscoveryStart::Found(vec![source, destination]));
            }
        }

        let request_id = self.next_request_id().await;
        let message = DiscoveryMessage::RouteRequest {
            destination,
            source,
            request_id,
            max_hops: self.max_hops,
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut pending = self.pending_requests.write().await;
        pending.insert(
            request_id,
            PendingRequest {
                destination,
                source,
                request_id,
                timestamp: now,
                responders: Vec::new(),
            },
        );

        Ok(DiscoveryStart::Pending {
            request_id,
            message,
        })
    }

    /// Legacy API — table lookup only (network discovery via `MeshManager::discover_route`).
    pub async fn discover_route(
        &self,
        destination: NodeId,
        source: NodeId,
    ) -> Result<Option<Vec<NodeId>>, MeshError> {
        match self.begin_discovery(destination, source).await? {
            DiscoveryStart::Found(route) => Ok(Some(route)),
            DiscoveryStart::Pending { .. } => Ok(None),
        }
    }

    /// Discover multiple routes in parallel (batch operation)
    ///
    /// Processes multiple route discoveries concurrently for better performance.
    /// Returns a HashMap of destination -> route (or None if not found).
    pub async fn discover_routes_batch(
        &self,
        destinations: &[NodeId],
        source: NodeId,
    ) -> Result<std::collections::HashMap<NodeId, Option<Vec<NodeId>>>, MeshError> {
        if destinations.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        // Discover all routes in parallel
        let futures: Vec<_> = destinations
            .iter()
            .map(|dest| self.discover_route(*dest, source))
            .collect();

        // Wait for all discoveries to complete
        let results = futures::future::join_all(futures).await;

        // Collect results into HashMap
        let mut route_map = std::collections::HashMap::new();
        for (dest, result) in destinations.iter().zip(results.into_iter()) {
            route_map.insert(*dest, result?);
        }

        Ok(route_map)
    }

    /// Handle route request from another node
    pub async fn handle_route_request(
        &self,
        request: &DiscoveryMessage,
        local_node_id: NodeId,
    ) -> Result<Option<DiscoveryMessage>, MeshError> {
        match request {
            DiscoveryMessage::RouteRequest {
                destination,
                source,
                request_id,
                max_hops: _,
            } => {
                if *destination == local_node_id {
                    let response = DiscoveryMessage::RouteResponse {
                        destination: *destination,
                        source: *source,
                        request_id: *request_id,
                        route: vec![*source, local_node_id],
                        cost: 0,
                    };
                    return Ok(Some(response));
                }

                if let Some(mut route) = self.routing_table.find_route(destination) {
                    if route.first() != Some(source) && !route.contains(source) {
                        route.insert(0, *source);
                    }
                    let response = DiscoveryMessage::RouteResponse {
                        destination: *destination,
                        source: *source,
                        request_id: *request_id,
                        route: route.clone(),
                        cost: route.len() as u64 * 10,
                    };
                    return Ok(Some(response));
                }

                if let Some(entry) = self.routing_table.get_route(destination) {
                    if entry.direct_address.is_some() {
                        let response = DiscoveryMessage::RouteResponse {
                            destination: *destination,
                            source: *source,
                            request_id: *request_id,
                            route: vec![*source, local_node_id, *destination],
                            cost: 10,
                        };
                        return Ok(Some(response));
                    }
                }

                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Handle route response
    pub async fn handle_route_response(
        &self,
        response: &DiscoveryMessage,
        from_node: NodeId,
    ) -> Result<(), MeshError> {
        match response {
            DiscoveryMessage::RouteResponse {
                destination,
                source: _,
                request_id,
                route,
                cost,
            } => {
                // Check if this is a response to a pending request
                let mut pending = self.pending_requests.write().await;
                if let Some(request) = pending.get_mut(request_id) {
                    // Add responder
                    request.responders.push(from_node);

                    // Create routing entry
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs();

                    let entry = RoutingEntry {
                        node_id: *destination,
                        direct_address: None,
                        next_hop: Some(route[1]), // Next hop is second node in route
                        route_path: route.clone(),
                        route_cost: *cost,
                        last_updated: now,
                        quality_score: 0.8, // Default quality for discovered routes
                    };

                    // Add route to routing table (lock-free with DashMap)
                    self.routing_table.add_route(entry);

                    info!(
                        "Route discovered: destination={:x?}, route_length={}, cost={}",
                        &destination[..8],
                        route.len(),
                        cost
                    );

                    pending.remove(request_id);

                    let mut waiters = self.waiters.write().await;
                    if let Some(tx) = waiters.remove(request_id) {
                        let _ = tx.send(route.clone());
                    }
                }

                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Handle route advertisement
    pub async fn handle_route_advertisement(
        &self,
        advertisement: &DiscoveryMessage,
        _from_node: NodeId,
    ) -> Result<(), MeshError> {
        match advertisement {
            DiscoveryMessage::RouteAdvertisement { routes, source } => {
                debug!(
                    "Received route advertisement: source={:x?}, routes={}",
                    &source[..8],
                    routes.len()
                );

                // Update routing table with advertised routes
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                for route_entry in routes {
                    // Create route path (source -> next_hop -> destination)
                    let route_path = vec![*source, route_entry.next_hop, route_entry.destination];

                    let entry = RoutingEntry {
                        node_id: route_entry.destination,
                        direct_address: None,
                        next_hop: Some(route_entry.next_hop),
                        route_path,
                        route_cost: route_entry.cost,
                        last_updated: now,
                        quality_score: 0.7, // Default quality for advertised routes
                    };

                    // Add or update route (lock-free with DashMap)
                    self.routing_table.add_route(entry);
                }

                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Clean up expired pending requests
    pub async fn cleanup_expired(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut pending = self.pending_requests.write().await;
        let mut expired = Vec::new();

        for (request_id, request) in pending.iter() {
            if now > request.timestamp + self.timeout_seconds {
                expired.push(*request_id);
            }
        }

        for request_id in &expired {
            pending.remove(request_id);
        }

        if !expired.is_empty() {
            debug!(
                "Cleaned up {} expired route discovery requests",
                expired.len()
            );
        }
    }
}
