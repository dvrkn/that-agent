//! Built-in channel adapter implementations.
//!
//! Each adapter implements the [`Channel`][crate::Channel] trait for a specific
//! communication platform.
//!
//! ## Available Adapters
//!
//! | Adapter | Outbound | Inbound | `ask_human` |
//! |---------|----------|---------|-------------|
//! | [`TelegramAdapter`] | ✓ | ✓ (long-poll) | ✓ |
//! | [`HttpAdapter`] | ✓ | ✓ (HTTP server) | ✓ |
//!
//! ## TUI Adapter
//!
//! The TUI adapter (`TuiChannel`) lives in `that-core::tui` to avoid a circular
//! dependency between `that-channels` and `that-core`.

pub mod http;
pub mod telegram;

pub use http::HttpAdapter;
pub use telegram::TelegramAdapter;
