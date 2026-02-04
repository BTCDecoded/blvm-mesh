//! Onion routing logic for blvm-onion

use blvm_mesh::NodeId;
use std::collections::HashSet;
use tracing::{debug, warn};

/// Onion routing configuration
pub struct OnionConfig {
    /// Default number of hops for onion routes
    pub default_hops: u8,
    /// Minimum number of hops
    pub min_hops: u8,
    /// Maximum number of hops
    pub max_hops: u8,
}

impl Default for OnionConfig {
    fn default() -> Self {
        Self {
            default_hops: 5,
            min_hops: 3,
            max_hops: 7,
        }
    }
}

/// Onion route builder
pub struct OnionRouteBuilder {
    config: OnionConfig,
}

impl OnionRouteBuilder {
    /// Create a new onion route builder
    pub fn new(config: OnionConfig) -> Self {
        Self { config }
    }

    /// Build an onion route from source to destination
    ///
    /// This is a simplified version that will be enhanced with actual
    /// route discovery via blvm-mesh.
    pub async fn build_route(
        &self,
        source: NodeId,
        destination: NodeId,
        available_nodes: Vec<NodeId>,
    ) -> Result<Vec<NodeId>, String> {
        let num_hops = self.config.default_hops;
        
        if num_hops < self.config.min_hops || num_hops > self.config.max_hops {
            return Err(format!(
                "Invalid hop count: {} (must be between {} and {})",
                num_hops, self.config.min_hops, self.config.max_hops
            ));
        }

        // Filter out source and destination from available nodes
        let mut candidates: Vec<NodeId> = available_nodes
            .into_iter()
            .filter(|&node| node != source && node != destination)
            .collect();

        if candidates.len() < (num_hops - 1) as usize {
            warn!(
                "Not enough intermediate nodes: need {}, have {}",
                num_hops - 1,
                candidates.len()
            );
            // Fall back to direct route if not enough nodes
            return Ok(vec![source, destination]);
        }

        // Shuffle and select intermediate nodes
        // TODO: Use proper randomization (rand crate)
        // For now, just take first N nodes
        let mut route = vec![source];
        
        // Select intermediate nodes (excluding source and destination)
        let intermediate_count = (num_hops - 1).min(candidates.len() as u8);
        for i in 0..intermediate_count {
            if let Some(node) = candidates.get(i as usize) {
                route.push(*node);
            }
        }
        
        route.push(destination);

        debug!(
            "Built onion route: {} hops (source -> {} intermediates -> destination)",
            route.len(),
            route.len() - 2
        );

        Ok(route)
    }

    /// Validate a route
    pub fn validate_route(&self, route: &[NodeId]) -> Result<(), String> {
        if route.len() < 2 {
            return Err("Route must have at least 2 nodes".to_string());
        }

        if route.len() > (self.config.max_hops + 1) as usize {
            return Err(format!(
                "Route too long: {} nodes (max: {})",
                route.len(),
                self.config.max_hops + 1
            ));
        }

        // Check for duplicates
        let mut seen = HashSet::new();
        for node in route {
            if seen.contains(node) {
                return Err("Route contains duplicate nodes".to_string());
            }
            seen.insert(*node);
        }

        Ok(())
    }
}

