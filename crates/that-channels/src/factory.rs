use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::adapters::{HttpAdapter, TelegramAdapter};
use crate::channel::{ChannelRef, InboundMessage};
use crate::config::{AdapterConfig, AdapterType, ChannelConfig};
use crate::router::ChannelRouter;

type FactoryFn = Arc<dyn Fn(&AdapterConfig, &str) -> Result<ChannelRef> + Send + Sync + 'static>;

/// Router build mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelBuildMode {
    /// Build only external/headless adapters.
    ///
    /// `tui` adapters are skipped because they require a live ratatui loop
    /// in `that-core`.
    Headless,
    /// Build every enabled adapter in the config.
    All,
}

/// Registry of adapter factories keyed by adapter type.
///
/// This removes adapter construction hardcoding from CLI code and gives
/// extension points for custom channel types.
#[derive(Clone, Default)]
pub struct ChannelFactoryRegistry {
    factories: HashMap<AdapterType, FactoryFn>,
}

impl ChannelFactoryRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry preloaded with built-in external adapters.
    ///
    /// Includes: `telegram`.
    /// Excludes: `tui` (lives in `that-core` to avoid circular dependency).
    pub fn with_builtin_adapters() -> Self {
        let mut registry = Self::new();
        registry.register_builtin_adapters();
        registry
    }

    /// Register a channel factory.
    pub fn register_mut<F>(&mut self, adapter_type: impl Into<AdapterType>, factory: F) -> &mut Self
    where
        F: Fn(&AdapterConfig, &str) -> Result<ChannelRef> + Send + Sync + 'static,
    {
        self.factories
            .insert(adapter_type.into(), Arc::new(factory));
        self
    }

    /// Register a channel factory (builder style).
    pub fn register<F>(mut self, adapter_type: impl Into<AdapterType>, factory: F) -> Self
    where
        F: Fn(&AdapterConfig, &str) -> Result<ChannelRef> + Send + Sync + 'static,
    {
        self.register_mut(adapter_type, factory);
        self
    }

    /// Build a channel router from config.
    pub fn build_router(
        &self,
        config: &ChannelConfig,
        mode: ChannelBuildMode,
    ) -> Result<(Arc<ChannelRouter>, mpsc::UnboundedReceiver<InboundMessage>)> {
        let mut channels: Vec<ChannelRef> = Vec::new();
        let mut primary_idx = 0usize;
        let mut id_counts: HashMap<String, usize> = HashMap::new();
        let primary = config
            .primary
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        for adapter_cfg in config.enabled_adapters() {
            if mode == ChannelBuildMode::Headless && adapter_cfg.adapter_type.is_tui() {
                continue;
            }

            let base_id = adapter_cfg.base_id();
            let entry = id_counts.entry(base_id.clone()).or_insert(0);
            *entry += 1;
            let id = if *entry == 1 {
                base_id.clone()
            } else {
                format!("{base_id}-{}", *entry)
            };

            let factory = self
                .factories
                .get(&adapter_cfg.adapter_type)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No channel factory registered for adapter type '{}'. \
                     Register one via ChannelFactoryRegistry::register[_mut]().",
                        adapter_cfg.adapter_type
                    )
                })?;

            let idx = channels.len();
            if let Some(primary) = primary {
                if primary == id
                    || primary == base_id
                    || primary == adapter_cfg.adapter_type.as_str()
                {
                    primary_idx = idx;
                }
            }

            channels.push(factory(adapter_cfg, &id)?);
        }

        if channels.is_empty() {
            anyhow::bail!(
                "No enabled channels configured for mode {:?}. \
                 Add channel adapters under [channels] in the agent config.",
                mode
            );
        }

        let (router, inbound_rx) = ChannelRouter::new(channels, primary_idx);
        Ok((Arc::new(router), inbound_rx))
    }

    fn register_builtin_adapters(&mut self) {
        self.register_mut(AdapterType::TELEGRAM, |cfg, id| {
            let token = required(&cfg.bot_token, "telegram", "bot_token")?;
            let chat_id = required(&cfg.chat_id, "telegram", "chat_id")?;
            Ok(Arc::new(TelegramAdapter::new(
                id.to_string(),
                token,
                chat_id,
                cfg.allowed_chats.clone(),
                cfg.allowed_senders.clone(),
            )))
        });

        self.register_mut(AdapterType::HTTP, |cfg, id| {
            let bind_addr = cfg
                .extra_value("bind_addr")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0.0:8080")
                .to_string();
            let auth_token = cfg
                .extra_value("auth_token")
                .and_then(|v| v.as_str())
                .map(String::from);
            let request_timeout_secs = cfg
                .extra_value("request_timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(300);
            Ok(Arc::new(HttpAdapter::new(
                id,
                &bind_addr,
                auth_token,
                request_timeout_secs,
            )))
        });
    }
}

fn required<'a>(value: &'a Option<String>, adapter: &str, field: &str) -> Result<&'a str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{adapter} adapter: {field} is required"))
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use async_trait::async_trait;
    use tokio::sync::mpsc;

    use crate::channel::{
        Channel, ChannelCapabilities, ChannelEvent, InboundMessage, MessageHandle, OutboundTarget,
    };

    use super::*;

    fn base_adapter(ty: &str) -> AdapterConfig {
        AdapterConfig {
            id: None,
            adapter_type: AdapterType::from(ty),
            enabled: true,
            bot_token: None,
            chat_id: None,
            allowed_chats: Vec::new(),
            allowed_senders: Vec::new(),
            extra: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn builds_primary_by_id() -> Result<()> {
        let mut a1 = base_adapter(AdapterType::TELEGRAM);
        a1.id = Some("main".into());
        a1.bot_token = Some("token-1".into());
        a1.chat_id = Some("chat-1".into());

        let mut a2 = base_adapter(AdapterType::TELEGRAM);
        a2.id = Some("ops".into());
        a2.bot_token = Some("token-2".into());
        a2.chat_id = Some("chat-2".into());

        let config = ChannelConfig {
            primary: Some("ops".into()),
            adapters: vec![a1, a2],
        };

        let registry = ChannelFactoryRegistry::with_builtin_adapters();
        let (router, _) = registry.build_router(&config, ChannelBuildMode::Headless)?;
        assert_eq!(router.primary_id(), "ops");
        Ok(())
    }

    #[test]
    fn auto_suffixes_duplicate_ids() -> Result<()> {
        let mut a1 = base_adapter(AdapterType::TELEGRAM);
        a1.bot_token = Some("token-1".into());
        a1.chat_id = Some("chat-1".into());

        let mut a2 = base_adapter(AdapterType::TELEGRAM);
        a2.bot_token = Some("token-2".into());
        a2.chat_id = Some("chat-2".into());

        let config = ChannelConfig {
            primary: None,
            adapters: vec![a1, a2],
        };

        let registry = ChannelFactoryRegistry::with_builtin_adapters();
        let (router, _) = registry.build_router(&config, ChannelBuildMode::Headless)?;
        assert_eq!(router.channel_ids(), "telegram,telegram-2");
        Ok(())
    }

    #[test]
    fn headless_skips_tui() {
        let config = ChannelConfig {
            primary: Some("tui".into()),
            adapters: vec![base_adapter(AdapterType::TUI)],
        };
        let registry = ChannelFactoryRegistry::with_builtin_adapters();
        match registry.build_router(&config, ChannelBuildMode::Headless) {
            Ok(_) => panic!("expected headless build to fail when only tui adapters are enabled"),
            Err(err) => assert!(err.to_string().contains("No enabled channels configured")),
        }
    }

    #[test]
    fn supports_custom_factory_registration() -> Result<()> {
        struct MockChannel {
            id: String,
        }

        #[async_trait]
        impl Channel for MockChannel {
            fn id(&self) -> &str {
                &self.id
            }

            fn capabilities(&self) -> ChannelCapabilities {
                ChannelCapabilities::default()
            }

            fn format_instructions(&self) -> Option<String> {
                None
            }

            async fn send_event(
                &self,
                _event: &ChannelEvent,
                _target: Option<&OutboundTarget>,
            ) -> Result<MessageHandle> {
                Ok(MessageHandle::default())
            }

            async fn ask_human(
                &self,
                _message: &str,
                _timeout: Option<u64>,
                _target: Option<&OutboundTarget>,
            ) -> Result<String> {
                Ok(String::new())
            }

            async fn start_listener(
                &self,
                _tx: mpsc::UnboundedSender<InboundMessage>,
            ) -> Result<()> {
                Ok(())
            }
        }

        let adapter = base_adapter("mock");
        let config = ChannelConfig {
            primary: Some("mock".into()),
            adapters: vec![adapter],
        };

        let registry = ChannelFactoryRegistry::new().register("mock", |cfg, id| {
            let id = cfg.id.clone().unwrap_or_else(|| id.to_string());
            Ok(Arc::new(MockChannel { id }))
        });
        let (router, _) = registry.build_router(&config, ChannelBuildMode::All)?;
        assert_eq!(router.channel_ids(), "mock");
        Ok(())
    }
}
