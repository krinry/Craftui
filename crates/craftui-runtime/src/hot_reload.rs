use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use craftui_core::{CraftError, NodeId, Result, monotonic_ms};
use craftui_dsl::{CuiFile, Diagnostic, Severity, resolve_file, resolve_source};
use craftui_reactive::Scope;
use craftui_widgets::WidgetTree;

use crate::dyn_value::{DynValue, DynamicStore};
use crate::interpreter::{
    DynEnv, build_element, eval_expr, eval_expr_tracked, expr_references_window,
};

/// Manages hot-reloading a `.cui` template into a live [`WidgetTree`].
pub struct HotReloadSession {
    pub state_scope: Scope,
    pub mount_scope: Option<Scope>,
    pub store: DynamicStore,
    pub last_source: String,
    pub last_reload_duration: Duration,
    pub last_diagnostics: Vec<Diagnostic>,
}

impl HotReloadSession {
    pub fn new(root_scope: Scope) -> Self {
        Self {
            state_scope: root_scope.child_scope(),
            mount_scope: None,
            store: DynamicStore::new(),
            last_source: String::new(),
            last_reload_duration: Duration::ZERO,
            last_diagnostics: Vec::new(),
        }
    }

    /// Parse, validate, and rebuild an inline `.cui` UI subtree while preserving live signal state.
    pub fn reload(
        &mut self,
        source: &str,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        self.reload_with_base(source, None, component_name, tree)
    }

    /// Parse, resolve relative `import` modules on disk, validate, and hot-reload a `.cui` file.
    pub fn reload_file(
        &mut self,
        path: impl AsRef<Path>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        let path_ref = path.as_ref();
        let src = std::fs::read_to_string(path_ref).map_err(|e| {
            CraftError::Runtime(format!("failed reading {}: {e}", path_ref.display()))
        })?;
        let t0 = monotonic_ms();
        let resolved = resolve_file(path_ref);
        let root =
            self.mount_resolved(resolved.merged, resolved.diagnostics, component_name, tree)?;
        self.last_source = src;
        self.last_reload_duration = Duration::from_millis(monotonic_ms().saturating_sub(t0));
        Ok(root)
    }

    /// Parse, resolve imports relative to `base_dir` (if provided), validate, and rebuild the subtree.
    pub fn reload_with_base(
        &mut self,
        source: &str,
        base_dir: Option<&Path>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        let t0 = monotonic_ms();
        let resolved = resolve_source(source, base_dir);
        let root =
            self.mount_resolved(resolved.merged, resolved.diagnostics, component_name, tree)?;
        self.last_source = source.to_string();
        self.last_reload_duration = Duration::from_millis(monotonic_ms().saturating_sub(t0));
        Ok(root)
    }

    fn mount_resolved(
        &mut self,
        merged: CuiFile,
        diags: Vec<Diagnostic>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        self.last_diagnostics = diags.clone();
        if diags.iter().any(|d| d.severity == Severity::Error) {
            let msg = diags
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(CraftError::Runtime(msg));
        }

        let comp = if let Some(name) = component_name {
            merged
                .components
                .iter()
                .find(|c| c.name == name)
                .or_else(|| merged.components.last())
        } else {
            merged.components.last()
        }
        .ok_or_else(|| CraftError::Runtime("no component found in .cui file".into()))?
        .clone();

        // Capture UI focus/caret/scroll snapshot BEFORE tearing down old subtree
        let ui_snap = tree.capture_ui_state();

        // Tear down old mounted tree nodes if present
        if let Some(old_root) = tree.root.take() {
            tree.remove_subtree(old_root);
        }
        if let Some(old_scope) = self.mount_scope.take() {
            old_scope.dispose();
        }

        let mount_scope = self.state_scope.child_scope();
        self.mount_scope = Some(mount_scope);

        let file_rc = Rc::new(merged);
        let base_env = DynEnv {
            file: file_rc,
            props: self.store.props.clone(),
            signal_locals: HashMap::new(),
            callbacks: self.store.callbacks.clone(),
            locals: HashMap::new(),
            theme_sig: tree.theme_signal,
            theme_ov_sig: tree.theme_override_signal,
            metrics_sig: tree.metrics_signal,
            children_slot: None,
        };

        // First pass: ensure all component properties exist in preserved DynamicStore
        for prop in &comp.properties {
            let env_cur = DynEnv {
                props: self.store.props.clone(),
                ..base_env.clone()
            };
            let init = prop
                .default_value
                .as_ref()
                .map(|e| eval_expr(e, &env_cur))
                .unwrap_or(DynValue::Null);
            self.store
                .get_or_create_prop(self.state_scope, &prop.name, init);
        }

        let full_env = DynEnv {
            props: self.store.props.clone(),
            ..base_env.clone()
        };

        // Second pass: install reactive Window effects with full component properties map
        for prop in &comp.properties {
            if let Some(ref def_expr) = prop.default_value
                && expr_references_window(def_expr)
            {
                let sig_c = self.store.props.get(&prop.name).copied().unwrap();
                let expr_c = def_expr.clone();
                let env_c = full_env.clone();
                mount_scope.effect(move || {
                    let v = eval_expr_tracked(&expr_c, &env_c, true);
                    sig_c.set(v);
                });
            }
        }

        let root_env = DynEnv {
            props: self.store.props.clone(),
            ..base_env
        };

        let root_node = if let Some(ref root_el) = comp.root {
            build_element(mount_scope, root_el, &root_env, tree)
        } else {
            tree.column(mount_scope)
        };

        tree.set_root(root_node);
        tree.restore_ui_state(&ui_snap);
        Ok(root_node)
    }
}
