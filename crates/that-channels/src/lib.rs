//! that-channels — Generic communication channel abstraction for the that-agent system.
//!
//! Provides:
//! - [`Channel`] trait — implemented by TUI and Telegram adapters
//! - [`ChannelRouter`] — fan-out broadcast and primary-channel human-ask routing
//! - [`ToolLogEvent`] — log event type for session transcript recording
//! - [`ChannelNotifyTool`] — built-in agent tool for mid-task human notifications
//! - [`ChannelConfig`] — TOML/env-var configuration for channel setup
//! - [`InboundRouter`] — routes inbound messages from external channels to agent sessions
//!
//! ## Circular Dependency Note
//!
//! `that-channels` does NOT depend on `that-core`. The TUI adapter lives in
//! `that-core::tui` (as `TuiChannel`) to avoid a circular dependency.

pub mod adapters;
pub mod channel;
pub mod config;
pub mod factory;
pub mod hook;
pub mod inbound;
pub mod router;
pub mod tool;

pub use channel::{
    BotCommand, Channel, ChannelCapabilities, ChannelEvent, ChannelRef, InboundMessage,
    MessageHandle, OutboundTarget,
};
pub use config::{AdapterConfig, AdapterType, ChannelConfig};
pub use factory::{ChannelBuildMode, ChannelFactoryRegistry};
pub use hook::ToolLogEvent;
pub use inbound::InboundRouter;
pub use router::ChannelRouter;
pub use tool::{ChannelNotifyTool, ChannelToolError};
