//! Error types for Mesh module

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MeshError {
    #[error("Module error: {0}")]
    ModuleError(String),
    
    #[error("Routing error: {0}")]
    RoutingError(String),
    
    #[error("Payment verification error: {0}")]
    PaymentError(String),
    
    #[error("Payment verification failed: {0}")]
    PaymentVerification(String),
    
    #[error("Classification error: {0}")]
    ClassificationError(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Invalid packet: {0}")]
    InvalidPacket(String),
    
    #[error("Replay detected: {0}")]
    ReplayDetected(String),
    
    #[error("Route not found: {0}")]
    RouteNotFound(String),
    
    #[error("Mesh disabled: {0}")]
    MeshDisabled(String),
}

