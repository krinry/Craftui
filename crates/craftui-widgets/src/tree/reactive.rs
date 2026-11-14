use super::*;

impl WidgetTree {
    /// Mount a reactive dynamic container whose subtree automatically rebuilds whenever `key_fn` changes.
    pub fn dynamic_slot<K: Clone + PartialEq + 'static>(
        &mut self,
        scope: Scope,
        layout: LayoutStyle,
        key_fn: impl Fn() -> K + 'static,
        builder: impl Fn(Scope, &K, &mut WidgetTree) -> NodeId + 'static,
    ) -> NodeId {
        let host = self.container(scope, layout, BoxStyle::new());
        let key_rc = Rc::new(key_fn);
        let key_for_eff = key_rc.clone();
        scope.effect(move || {
            let _ = key_for_eff();
            mark_node_dirty(host, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
        });

        let mut last_key: Option<K> = None;
        let mut child_scope_opt: Option<Scope> = None;
        let sync_fn: DynamicSlotSync = Box::new(move |host_id, tree| {
            let next_key = key_rc();
            if last_key.as_ref() == Some(&next_key) {
                return false;
            }
            last_key = Some(next_key.clone());

            let old_children = tree
                .nodes
                .get(host_id)
                .map(|n| n.children.clone())
                .unwrap_or_default();
            for oc in old_children {
                tree.remove_subtree(oc);
            }
            if let Some(old_sc) = child_scope_opt.take() {
                old_sc.dispose();
            }

            let new_scope = scope.child_scope();
            child_scope_opt = Some(new_scope);
            let child = builder(new_scope, &next_key, tree);
            tree.set_children(host_id, vec![child]);
            true
        });

        self.dynamic_hosts.insert(host, sync_fn);
        self.sync_dynamic_slots();
        host
    }

    /// Mount a [`Router`]-driven view container playing [`RouterTransition`] whenever routes switch.
    pub fn animated_route_slot<R: Clone + PartialEq + 'static>(
        &mut self,
        scope: Scope,
        layout: LayoutStyle,
        router: Router<R>,
        transition: RouterTransition,
        builder: impl Fn(Scope, &R, &mut WidgetTree) -> NodeId + 'static,
    ) -> NodeId {
        let host = self.container(scope, layout, BoxStyle::new());
        scope.effect(move || {
            let _ = router.current();
            let _ = router.depth();
            mark_node_dirty(host, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
        });

        let mut last_route: Option<R> = None;
        let mut last_depth: usize = router.depth();
        let mut child_scope_opt: Option<Scope> = None;
        let sync_fn: DynamicSlotSync = Box::new(move |host_id, tree| {
            let next_route = router.current();
            let next_depth = router.depth();
            if last_route.as_ref() == Some(&next_route) {
                return false;
            }
            let is_initial = last_route.is_none();
            let is_pop = next_depth < last_depth;
            last_route = Some(next_route.clone());
            last_depth = next_depth;

            let old_children = tree
                .nodes
                .get(host_id)
                .map(|n| n.children.clone())
                .unwrap_or_default();
            for oc in old_children {
                tree.remove_subtree(oc);
            }
            if let Some(old_sc) = child_scope_opt.take() {
                old_sc.dispose();
            }

            let new_scope = scope.child_scope();
            child_scope_opt = Some(new_scope);
            let child = builder(new_scope, &next_route, tree);
            tree.set_children(host_id, vec![child]);

            if !is_initial && transition != RouterTransition::None {
                let fade_curve = TransitionCurve::tween(0.18, Easing::EaseOut);
                let move_curve = TransitionCurve::spring(340.0, 30.0);
                let mut anim = NodeAnimState::default();
                match transition {
                    RouterTransition::None => {}
                    RouterTransition::Fade => {
                        anim.curves.insert(AnimatableProp::Opacity, fade_curve);
                        let mut op = AnimatedFloat::new(0.08, fade_curve);
                        op.set_target(1.0);
                        anim.opacity = Some(op);
                    }
                    RouterTransition::SlideLeft | RouterTransition::SlideRight => {
                        let base_offset = if matches!(transition, RouterTransition::SlideLeft) {
                            36.0
                        } else {
                            -36.0
                        };
                        let start_x = if is_pop { -base_offset } else { base_offset };
                        anim.curves.insert(AnimatableProp::Opacity, fade_curve);
                        anim.curves.insert(AnimatableProp::TranslateX, move_curve);
                        let mut op = AnimatedFloat::new(0.12, fade_curve);
                        op.set_target(1.0);
                        let mut tx = AnimatedFloat::new(start_x, move_curve);
                        tx.set_target(0.0);
                        anim.opacity = Some(op);
                        anim.translate_x = Some(tx);
                    }
                    RouterTransition::ScaleFade => {
                        anim.curves.insert(AnimatableProp::Opacity, fade_curve);
                        anim.curves.insert(AnimatableProp::ScaleX, move_curve);
                        anim.curves.insert(AnimatableProp::ScaleY, move_curve);
                        let mut op = AnimatedFloat::new(0.12, fade_curve);
                        op.set_target(1.0);
                        let mut sx = AnimatedFloat::new(0.94, move_curve);
                        sx.set_target(1.0);
                        let mut sy = AnimatedFloat::new(0.94, move_curve);
                        sy.set_target(1.0);
                        anim.opacity = Some(op);
                        anim.scale_x = Some(sx);
                        anim.scale_y = Some(sy);
                    }
                }
                tree.node_anims.insert(child, anim);
            }
            true
        });

        self.dynamic_hosts.insert(host, sync_fn);
        self.sync_dynamic_slots();
        host
    }

    /// Mount a keyed reactive list container reconciling items when `model` changes.
    /// Items whose key persists retain their existing subtree `NodeId` and [`Scope`].
    pub fn reactive_for<T, K, M, KF, B>(
        &mut self,
        scope: Scope,
        layout: LayoutStyle,
        model: M,
        key_fn: KF,
        builder: B,
    ) -> NodeId
    where
        T: Clone + 'static,
        K: Eq + Hash + Clone + 'static,
        M: Fn() -> Vec<T> + 'static,
        KF: Fn(usize, &T) -> K + 'static,
        B: Fn(Scope, T, usize, &mut WidgetTree) -> NodeId + 'static,
    {
        let host = self.container(scope, layout, BoxStyle::new());
        let model_rc = Rc::new(model);
        let model_eff = model_rc.clone();
        scope.effect(move || {
            let _ = model_eff();
            mark_node_dirty(host, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
        });

        let mut active_by_key: HashMap<(K, usize), (NodeId, Scope)> = HashMap::new();
        let mut last_keys: Option<Vec<(K, usize)>> = None;

        let sync_fn: DynamicSlotSync = Box::new(move |host_id, tree| {
            let items = model_rc();
            let mut seen_counts: HashMap<K, usize> = HashMap::new();
            let new_keys: Vec<(K, usize)> = items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    let base = key_fn(i, it);
                    let occ = seen_counts.entry(base.clone()).or_insert(0);
                    let cur = *occ;
                    *occ += 1;
                    (base, cur)
                })
                .collect();

            if last_keys.as_ref() == Some(&new_keys) {
                return false;
            }
            last_keys = Some(new_keys.clone());

            let keep_set: HashSet<(K, usize)> = new_keys.iter().cloned().collect();
            let removed_keys: Vec<(K, usize)> = active_by_key
                .keys()
                .filter(|k| !keep_set.contains(*k))
                .cloned()
                .collect();

            for rk in removed_keys {
                if let Some((node_id, item_scope)) = active_by_key.remove(&rk) {
                    tree.remove_subtree(node_id);
                    item_scope.dispose();
                }
            }

            let mut ordered = Vec::with_capacity(items.len());
            for (idx, (item, key)) in items.into_iter().zip(new_keys).enumerate() {
                if let Some(&(existing, _)) = active_by_key.get(&key) {
                    ordered.push(existing);
                } else {
                    let item_scope = scope.child_scope();
                    let child = builder(item_scope, item, idx, tree);
                    active_by_key.insert(key, (child, item_scope));
                    ordered.push(child);
                }
            }

            tree.set_children(host_id, ordered);
            true
        });

        self.dynamic_hosts.insert(host, sync_fn);
        self.sync_dynamic_slots();
        host
    }

    /// Mount a keyed reactive list container reconciling items when `model` changes.
    /// Existing keyed subtrees keep their `NodeId` and [`Scope`] while receiving updated
    /// `Signal<T>` and `Signal<usize>` values in place without rebuilding widgets.
    pub fn reactive_for_keyed<T, K, M, KF, B>(
        &mut self,
        scope: Scope,
        layout: LayoutStyle,
        model: M,
        key_fn: KF,
        builder: B,
    ) -> NodeId
    where
        T: Clone + PartialEq + 'static,
        K: Eq + Hash + Clone + 'static,
        M: Fn() -> Vec<T> + 'static,
        KF: Fn(usize, &T) -> K + 'static,
        B: Fn(Scope, Signal<T>, Signal<usize>, &mut WidgetTree) -> NodeId + 'static,
    {
        let host = self.container(scope, layout, BoxStyle::new());
        let model_rc = Rc::new(model);
        let model_eff = model_rc.clone();
        scope.effect(move || {
            let _ = model_eff();
            mark_node_dirty(host, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
        });

        type KeyedForSlot<K, T> = HashMap<(K, usize), (NodeId, Scope, Signal<T>, Signal<usize>)>;
        let mut active_by_key: KeyedForSlot<K, T> = HashMap::new();
        let mut last_keys: Option<Vec<(K, usize)>> = None;

        let sync_fn: DynamicSlotSync = Box::new(move |host_id, tree| {
            let items = model_rc();
            let mut seen_counts: HashMap<K, usize> = HashMap::new();
            let new_keys: Vec<(K, usize)> = items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    let base = key_fn(i, it);
                    let occ = seen_counts.entry(base.clone()).or_insert(0);
                    let cur = *occ;
                    *occ += 1;
                    (base, cur)
                })
                .collect();

            if last_keys.as_ref() == Some(&new_keys) {
                for (idx, (item, key)) in items.into_iter().zip(new_keys).enumerate() {
                    if let Some(&(_, _, item_sig, idx_sig)) = active_by_key.get(&key) {
                        if item_sig.get_untracked() != item {
                            item_sig.set(item);
                        }
                        if idx_sig.get_untracked() != idx {
                            idx_sig.set(idx);
                        }
                    }
                }
                return false;
            }
            last_keys = Some(new_keys.clone());

            let keep_set: HashSet<(K, usize)> = new_keys.iter().cloned().collect();
            let removed_keys: Vec<(K, usize)> = active_by_key
                .keys()
                .filter(|k| !keep_set.contains(*k))
                .cloned()
                .collect();

            for rk in removed_keys {
                if let Some((node_id, item_scope, _, _)) = active_by_key.remove(&rk) {
                    tree.remove_subtree(node_id);
                    item_scope.dispose();
                }
            }

            let mut ordered = Vec::with_capacity(items.len());
            for (idx, (item, key)) in items.into_iter().zip(new_keys).enumerate() {
                if let Some(&(existing, _, item_sig, idx_sig)) = active_by_key.get(&key) {
                    if item_sig.get_untracked() != item {
                        item_sig.set(item);
                    }
                    if idx_sig.get_untracked() != idx {
                        idx_sig.set(idx);
                    }
                    ordered.push(existing);
                } else {
                    let item_scope = scope.child_scope();
                    let item_sig = item_scope.signal(item);
                    let idx_sig = item_scope.signal(idx);
                    let child = builder(item_scope, item_sig, idx_sig, tree);
                    active_by_key.insert(key, (child, item_scope, item_sig, idx_sig));
                    ordered.push(child);
                }
            }

            tree.set_children(host_id, ordered);
            true
        });

        self.dynamic_hosts.insert(host, sync_fn);
        self.sync_dynamic_slots();
        host
    }

    pub(crate) fn sync_dynamic_slots(&mut self) -> bool {
        let mut changed = false;
        let keys: Vec<NodeId> = self.dynamic_hosts.keys().collect();
        for host_id in keys {
            if !self.nodes.contains_key(host_id) {
                continue;
            }
            if let Some(mut sync_fn) = self.dynamic_hosts.remove(host_id) {
                if sync_fn(host_id, self) {
                    changed = true;
                }
                if self.nodes.contains_key(host_id) {
                    self.dynamic_hosts.insert(host_id, sync_fn);
                }
            }
        }
        changed
    }

    pub(crate) fn sync_dynamic_props(&mut self, drained_masks: &HashMap<NodeId, DirtyFlags>) {
        if drained_masks.is_empty() || self.dynamic_prop_updaters.is_empty() {
            return;
        }
        let ids: Vec<NodeId> = self
            .dynamic_prop_updaters
            .keys()
            .filter(|id| drained_masks.contains_key(id))
            .collect();
        for id in ids {
            let Some(&flags) = drained_masks.get(&id) else {
                continue;
            };
            if !self.nodes.contains_key(id) {
                continue;
            }
            if let Some(mut list) = self.dynamic_prop_updaters.remove(id) {
                for (mask, updater) in &mut list {
                    if !(flags & *mask).is_empty() {
                        updater(id, self);
                    }
                }
                if self.nodes.contains_key(id) {
                    self.dynamic_prop_updaters.insert(id, list);
                }
            }
        }
    }
}
