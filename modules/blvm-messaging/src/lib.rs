//! blvm-messaging library - P2P messaging via payment-gated mesh

pub mod message;
pub mod module;

pub use message::{Message, MessagingService};
pub use module::MessagingModule;
