//! blvm-mining-pool library

pub mod module;
pub mod pool;

pub use module::MiningPoolModule;
pub use pool::{BlockTemplate, PoolCoordinator, PoolMember, PoolMessage, PoolStats};
