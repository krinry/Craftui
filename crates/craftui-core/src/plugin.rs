//! Flutter-compatible `MethodChannel` / `PluginRegistry` bridge for native platform plugins.
//!
//! Enables Rust applications to invoke native Android (Kotlin/Java), iOS (Swift/ObjC),
//! or Desktop plugin handlers—including handlers ported from `pub.dev` platform plugins—
//! over a uniform JSON method-call contract.

use crate::{CraftError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Structured method invocation sent across a named plugin channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MethodCall {
    pub channel: String,
    pub method: String,
    pub args: serde_json::Value,
}

impl MethodCall {
    pub fn new(
        channel: impl Into<String>,
        method: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self {
            channel: channel.into(),
            method: method.into(),
            args,
        }
    }
}

/// Trait implemented by native or cross-platform CraftUI plugins.
pub trait CraftPlugin: Send + Sync {
    /// Unique channel name (for example `"dev.craftui.camera"` or `"plugins.flutter.io/url_launcher"`).
    fn name(&self) -> &str;

    /// Handle a synchronous method call on this channel.
    fn handle(&self, call: &MethodCall) -> Result<serde_json::Value>;
}

/// Thread-safe registry mapping channel names to [`CraftPlugin`] implementations.
#[derive(Clone, Default)]
pub struct PluginRegistry {
    channels: Arc<RwLock<HashMap<String, Arc<dyn CraftPlugin>>>>,
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.channels.read().map(|g| g.len()).unwrap_or(0);
        f.debug_struct("PluginRegistry")
            .field("channel_count", &count)
            .finish()
    }
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a plugin under its [`CraftPlugin::name`]. Replaces any previous handler with the same name.
    pub fn register<P: CraftPlugin + 'static>(&self, plugin: P) {
        self.register_arc(Arc::new(plugin));
    }

    /// Register a pre-wrapped `Arc<dyn CraftPlugin>`.
    pub fn register_arc(&self, plugin: Arc<dyn CraftPlugin>) {
        if let Ok(mut guard) = self.channels.write() {
            guard.insert(plugin.name().to_string(), plugin);
        }
    }

    /// Unregister a plugin channel by name. Returns `true` if a handler was removed.
    pub fn unregister(&self, channel: &str) -> bool {
        self.channels
            .write()
            .map(|mut guard| guard.remove(channel).is_some())
            .unwrap_or(false)
    }

    /// Check whether a channel has a registered plugin handler.
    pub fn has_channel(&self, channel: &str) -> bool {
        self.channels
            .read()
            .map(|guard| guard.contains_key(channel))
            .unwrap_or(false)
    }

    /// List all currently registered channel names.
    pub fn channels(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .channels
            .read()
            .map(|guard| guard.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Invoke a method on a registered plugin channel.
    pub fn invoke(
        &self,
        channel: impl Into<String>,
        method: impl Into<String>,
        args: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let channel = channel.into();
        let handler = self
            .channels
            .read()
            .map_err(|_| CraftError::Platform("plugin registry lock poisoned".into()))?
            .get(&channel)
            .cloned()
            .ok_or_else(|| {
                CraftError::Platform(format!("no plugin registered for channel `{channel}`"))
            })?;

        let call = MethodCall::new(channel, method, args);
        handler.handle(&call)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct EchoPlugin;

    impl CraftPlugin for EchoPlugin {
        fn name(&self) -> &str {
            "dev.craftui.echo"
        }

        fn handle(&self, call: &MethodCall) -> Result<serde_json::Value> {
            match call.method.as_str() {
                "ping" => Ok(json!({ "pong": call.args })),
                other => Err(CraftError::Platform(format!("unknown method `{other}`"))),
            }
        }
    }

    #[test]
    fn register_and_invoke_method_channel() {
        let registry = PluginRegistry::new();
        assert!(!registry.has_channel("dev.craftui.echo"));

        registry.register(EchoPlugin);
        assert!(registry.has_channel("dev.craftui.echo"));
        assert_eq!(registry.channels(), vec!["dev.craftui.echo".to_string()]);

        let res = registry
            .invoke("dev.craftui.echo", "ping", json!({ "msg": "hello" }))
            .expect("echo plugin should succeed");
        assert_eq!(res, json!({ "pong": { "msg": "hello" } }));

        assert!(
            registry
                .invoke("dev.craftui.missing", "ping", json!({}))
                .is_err()
        );
        assert!(registry.unregister("dev.craftui.echo"));
        assert!(!registry.has_channel("dev.craftui.echo"));
    }
}
