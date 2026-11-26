use crate::anim::{AnimatableProp, AnimatedColor, AnimatedFloat, Easing, Spring, TransitionCurve};
use crate::gesture::{GestureArena, RecognizedGesture, ScrollPhysics};
use crate::inspector::InspectorState;
use crate::router::{Router, RouterTransition};
use crate::toast::{ToastItem, ToastManager, ToastSeverity};
use crate::widgets::{
    BoolCallback, BoxStyle, CanvasPointerFn, CardVariant, CustomWidget, FloatCallback,
    HoverCallback, IndexCallback, PinchRotateCallback, ReactiveValue, TapHandler,
    ToastDismissCallback, WidgetKind,
};
use craftui_a11y::{A11yAction, A11yBridge, A11yMetadata, A11yRole, A11ySnapshotNode};
use craftui_core::{
    Alignment, Axis, BlendMode, BoxShadow, Brush, Color, CornerRadii, CursorIcon, Direction,
    DirtyFlags, Fit, Insets, KeyCode, KeyEvent, Length, NodeId, PlatformServices, PlatformViewId,
    Point, PointerEvent, PointerPhase, Rect, Size, TextureId, ThemeMode, ThemeOverride,
    ThemeTokens, Transform2D, UiEvent, WindowMetrics,
};
use craftui_layout::{LayoutEngine, LayoutStyle};
use craftui_reactive::{Scope, Signal, drain_dirty_nodes, mark_node_dirty};
use craftui_render::{PaintCommand, PaintScene, builtin_icon_svg, paint_svg_icon};
use craftui_text::{RichText, TextEditorState, TextEngine, TextStyle};
use slotmap::{SecondaryMap, SlotMap};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

/// Single node in the retained UI tree.
pub struct WidgetNode {
    pub id: NodeId,
    pub debug_name: &'static str,
    pub kind: WidgetKind,
    pub layout: LayoutStyle,
    pub box_style: BoxStyle,
    pub a11y: A11yMetadata,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub scope: Scope,
    pub dirty: DirtyFlags,
    pub on_tap: Option<TapHandler>,
    pub on_double_tap: Option<TapHandler>,
    pub on_long_press: Option<TapHandler>,
    pub on_hover: Option<HoverCallback>,
    pub on_pinch_rotate: Option<PinchRotateCallback>,
    /// Optional reactive visibility predicate evaluated during layout.
    pub visible_when: Option<ReactiveValue<bool>>,
    /// Optional reactive disabled predicate evaluated across interactive widgets.
    pub disabled: Option<ReactiveValue<bool>>,
    cached_text: String,
    cached_style: TextStyle,
    taffy_collapsed_override: bool,
    taffy_hidden_by_parent: bool,
}

impl WidgetNode {
    pub(crate) fn new(
        id: NodeId,
        debug_name: &'static str,
        kind: WidgetKind,
        layout: LayoutStyle,
        scope: Scope,
    ) -> Self {
        Self {
            id,
            debug_name,
            kind,
            layout,
            box_style: BoxStyle::new(),
            a11y: A11yMetadata::default(),
            parent: None,
            children: Vec::new(),
            scope,
            dirty: DirtyFlags::ALL,
            on_tap: None,
            on_double_tap: None,
            on_long_press: None,
            on_hover: None,
            on_pinch_rotate: None,
            visible_when: None,
            disabled: None,
            cached_text: String::new(),
            cached_style: TextStyle::default(),
            taffy_collapsed_override: false,
            taffy_hidden_by_parent: false,
        }
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled.as_ref().is_some_and(|d| d.get())
            || matches!(&self.kind, WidgetKind::Button { disabled, .. } if disabled.get())
    }
}

/// Callback type used by `VirtualList` and `VirtualGrid` to materialize visible items lazily.
pub type VirtualItemFactory = Rc<dyn Fn(Scope, usize, &mut WidgetTree) -> NodeId>;

type DynamicSlotSync = Box<dyn FnMut(NodeId, &mut WidgetTree) -> bool>;
type DynamicPropUpdater = (DirtyFlags, Box<dyn FnMut(NodeId, &mut WidgetTree)>);

/// Portable snapshot of transient widget UI state (focus path, scroll offsets, text cursors)
/// used by hot-reload sessions to preserve user state across live template rebuilds.
#[derive(Debug, Clone, Default)]
pub struct TreeUiSnapshot {
    pub focused_path: Option<Vec<usize>>,
    pub scroll_offsets: Vec<(Vec<usize>, f32)>,
    pub editor_states: Vec<(Vec<usize>, usize, usize)>,
    pub select_open_paths: Vec<Vec<usize>>,
}

const CARET_BLINK_INTERVAL_MS: u64 = 530;

#[derive(Default)]
pub(crate) struct PropEffectScopes {
    box_bg: Option<Scope>,
    box_border_color: Option<Scope>,
    divider_color: Option<Scope>,
    button_fg: Option<Scope>,
    spinner_color: Option<Scope>,
    badge_fg: Option<Scope>,
    badge_bg: Option<Scope>,
    badge_icon: Option<Scope>,
    disabled: Option<Scope>,
    visible_when: Option<Scope>,
    dynamic_props: Vec<Scope>,
}

#[derive(Debug, Clone, Default)]
struct NodeAnimState {
    curves: HashMap<AnimatableProp, TransitionCurve>,
    opacity: Option<AnimatedFloat>,
    translate_x: Option<AnimatedFloat>,
    translate_y: Option<AnimatedFloat>,
    scale_x: Option<AnimatedFloat>,
    scale_y: Option<AnimatedFloat>,
    rotation: Option<AnimatedFloat>,
    bg_color: Option<AnimatedColor>,
    border_color: Option<AnimatedColor>,
}

impl NodeAnimState {
    pub(crate) fn is_animating(&self) -> bool {
        self.opacity
            .as_ref()
            .is_some_and(AnimatedFloat::is_animating)
            || self
                .translate_x
                .as_ref()
                .is_some_and(AnimatedFloat::is_animating)
            || self
                .translate_y
                .as_ref()
                .is_some_and(AnimatedFloat::is_animating)
            || self
                .scale_x
                .as_ref()
                .is_some_and(AnimatedFloat::is_animating)
            || self
                .scale_y
                .as_ref()
                .is_some_and(AnimatedFloat::is_animating)
            || self
                .rotation
                .as_ref()
                .is_some_and(AnimatedFloat::is_animating)
            || self
                .bg_color
                .as_ref()
                .is_some_and(AnimatedColor::is_animating)
            || self
                .border_color
                .as_ref()
                .is_some_and(AnimatedColor::is_animating)
    }

    pub(crate) fn step(&mut self, dt_sec: f32) -> (bool, bool) {
        let mut changed = false;
        for opt in [
            &mut self.opacity,
            &mut self.translate_x,
            &mut self.translate_y,
            &mut self.scale_x,
            &mut self.scale_y,
            &mut self.rotation,
        ]
        .into_iter()
        .flatten()
        {
            if opt.step(dt_sec) {
                changed = true;
            }
        }
        if let Some(bg) = self.bg_color.as_mut()
            && bg.step(dt_sec)
        {
            changed = true;
        }
        if let Some(bc) = self.border_color.as_mut()
            && bc.step(dt_sec)
        {
            changed = true;
        }
        (changed, self.is_animating())
    }
}

#[derive(Debug, Clone)]
enum ControlVisualAnim {
    Button {
        hover_t: AnimatedFloat,
        press_t: AnimatedFloat,
    },
    Switch {
        knob_t: AnimatedFloat,
    },
    Checkbox {
        check_t: AnimatedFloat,
    },
    Tabs {
        active_t: AnimatedFloat,
    },
    SegmentedControl {
        active_t: AnimatedFloat,
    },
    BottomNavBar {
        active_t: AnimatedFloat,
    },
    FloatingActionButton {
        hover_t: AnimatedFloat,
        press_t: AnimatedFloat,
    },
    Slider {
        frac_t: AnimatedFloat,
        hover_t: AnimatedFloat,
        drag_t: AnimatedFloat,
        dragging: bool,
    },
    Overlay {
        was_open: bool,
        reveal: AnimatedFloat,
    },
}

impl ControlVisualAnim {
    pub(crate) fn is_animating(&self) -> bool {
        match self {
            Self::Button { hover_t, press_t } | Self::FloatingActionButton { hover_t, press_t } => {
                hover_t.is_animating() || press_t.is_animating()
            }
            Self::Switch { knob_t } => knob_t.is_animating(),
            Self::Checkbox { check_t } => check_t.is_animating(),
            Self::Tabs { active_t }
            | Self::SegmentedControl { active_t }
            | Self::BottomNavBar { active_t } => active_t.is_animating(),
            Self::Slider {
                frac_t,
                hover_t,
                drag_t,
                ..
            } => frac_t.is_animating() || hover_t.is_animating() || drag_t.is_animating(),
            Self::Overlay { reveal, .. } => reveal.is_animating(),
        }
    }

    pub(crate) fn step(&mut self, dt_sec: f32) -> bool {
        match self {
            Self::Button { hover_t, press_t } | Self::FloatingActionButton { hover_t, press_t } => {
                let a = hover_t.step(dt_sec);
                let b = press_t.step(dt_sec);
                a || b
            }
            Self::Switch { knob_t } => knob_t.step(dt_sec),
            Self::Checkbox { check_t } => check_t.step(dt_sec),
            Self::Tabs { active_t }
            | Self::SegmentedControl { active_t }
            | Self::BottomNavBar { active_t } => active_t.step(dt_sec),
            Self::Slider {
                frac_t,
                hover_t,
                drag_t,
                ..
            } => {
                let a = frac_t.step(dt_sec);
                let b = hover_t.step(dt_sec);
                let c = drag_t.step(dt_sec);
                a || b || c
            }
            Self::Overlay { reveal, .. } => reveal.step(dt_sec),
        }
    }
}

struct VirtualListHost {
    factory: VirtualItemFactory,
    active_items: HashMap<usize, (NodeId, Scope)>,
    last_range: Option<(usize, usize)>,
}

/// Central retained widget tree managing layout, text, reactivity, gestures, overlays, and painting.
pub struct WidgetTree {
    pub nodes: SlotMap<NodeId, WidgetNode>,
    pub root: Option<NodeId>,
    pub layout_engine: LayoutEngine,
    pub text_engine: TextEngine,
    pub gesture_arena: GestureArena,
    pub scroll_physics: ScrollPhysics,
    pub a11y_bridge: A11yBridge,
    pub inspector: InspectorState,
    pub root_scope: Scope,
    pub theme_signal: Signal<ThemeTokens>,
    pub theme_override_signal: Signal<ThemeOverride>,
    design_preset: craftui_core::DesignSystem,
    pub metrics_signal: Signal<WindowMetrics>,
    pub services: PlatformServices,
    pub direction: Direction,
    pub direction_signal: Signal<Direction>,
    pub focused: Option<NodeId>,
    pub focus_visible: bool,
    pub hovered: Option<NodeId>,
    last_pointer_pos: Option<Point>,
    pub(crate) active_drag: Option<(NodeId, String, Point, bool, craftui_core::PointerId)>,
    pub(crate) reorder_drag_owner: Option<(NodeId, craftui_core::PointerId, Option<String>)>,
    pub metrics: WindowMetrics,
    pub(crate) last_layout_metrics: Option<WindowMetrics>,
    pub clipboard_text: String,
    virtual_hosts: SecondaryMap<NodeId, VirtualListHost>,
    dynamic_hosts: SecondaryMap<NodeId, DynamicSlotSync>,
    dynamic_prop_updaters: SecondaryMap<NodeId, Vec<DynamicPropUpdater>>,
    prop_scopes: SecondaryMap<NodeId, PropEffectScopes>,
    node_anims: SecondaryMap<NodeId, NodeAnimState>,
    control_anims: SecondaryMap<NodeId, ControlVisualAnim>,
    named_nodes: HashMap<String, NodeId>,
    theme_spring: Option<(ThemeTokens, ThemeTokens, Spring)>,
    caret_visible: bool,
    caret_next_toggle_ms: Option<u64>,
    last_caret_focused: Option<NodeId>,
}

impl WidgetTree {
    pub fn new(root_scope: Scope) -> Self {
        let services = root_scope
            .use_context::<PlatformServices>()
            .unwrap_or_else(PlatformServices::in_memory);
        let initial_theme = ThemeTokens::dark();
        let theme_signal = root_scope.signal(initial_theme);
        let theme_override_signal = root_scope.signal(ThemeOverride::default());
        let metrics_signal = root_scope.signal(WindowMetrics::default());
        let direction_signal = root_scope.signal(Direction::Ltr);
        root_scope.provide_context(theme_signal);
        root_scope.provide_context(theme_override_signal);
        root_scope.provide_context(metrics_signal);
        root_scope.provide_context(direction_signal);
        root_scope.provide_context(services.clone());
        Self {
            nodes: SlotMap::with_key(),
            root: None,
            layout_engine: LayoutEngine::new(),
            text_engine: TextEngine::new(),
            gesture_arena: GestureArena::new(),
            scroll_physics: ScrollPhysics::default(),
            a11y_bridge: A11yBridge::new("CraftUI App"),
            inspector: InspectorState::default(),
            root_scope,
            theme_signal,
            theme_override_signal,
            design_preset: craftui_core::DesignSystem::Material3,
            metrics_signal,
            services,
            direction: Direction::Ltr,
            direction_signal,
            focused: None,
            focus_visible: false,
            hovered: None,
            last_pointer_pos: None,
            active_drag: None,
            reorder_drag_owner: None,
            metrics: WindowMetrics::default(),
            last_layout_metrics: None,
            clipboard_text: String::new(),
            virtual_hosts: SecondaryMap::new(),
            dynamic_hosts: SecondaryMap::new(),
            dynamic_prop_updaters: SecondaryMap::new(),
            prop_scopes: SecondaryMap::new(),
            node_anims: SecondaryMap::new(),
            control_anims: SecondaryMap::new(),
            named_nodes: HashMap::new(),
            theme_spring: None,
            caret_visible: true,
            caret_next_toggle_ms: None,
            last_caret_focused: None,
        }
    }

    pub fn set_platform_services(&mut self, services: PlatformServices) {
        self.services = services.clone();
        self.root_scope.provide_context(services);
    }

    pub fn theme(&self) -> ThemeTokens {
        self.theme_signal.get()
    }

    pub fn theme_override(&self) -> ThemeOverride {
        self.theme_override_signal.get()
    }

    /// Look up either a built-in semantic color token or a custom override token by name.
    pub fn resolve_theme_color(&self, token_name: &str) -> Option<Color> {
        let theme = self.theme_signal.get();
        if let Some(c) = theme.token_color(token_name) {
            return Some(c);
        }
        self.theme_override_signal.with(|ov| ov.color(token_name))
    }

    /// Apply a custom brand palette override immediately onto the active theme.
    pub fn set_theme_override(&mut self, override_spec: ThemeOverride) {
        self.theme_override_signal.set(override_spec.clone());
        let base =
            ThemeTokens::from_preset(self.design_preset, self.theme_signal.get_untracked().mode);
        self.theme_signal.set(base.applied(&override_spec));
    }

    /// Directly set the active theme tokens (e.g. dynamic Material 3 palette).
    pub fn set_theme(&mut self, theme: ThemeTokens) {
        self.theme_signal.set(theme);
    }

    /// Animate smoothly between theme palettes (Light, Dark, AMOLED, HighContrast), preserving custom overrides.
    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        let from = self.theme_signal.get();
        let ov = self.theme_override_signal.get_untracked();
        let to = ThemeTokens::from_preset(self.design_preset, mode).applied(&ov);
        let mut spring = Spring::new(0.0, 260.0, 28.0);
        spring.set_target(1.0);
        self.theme_spring = Some((from, to, spring));
        for node in self.nodes.values_mut() {
            node.dirty |= DirtyFlags::PAINT;
        }
    }

    /// Apply a native adaptive [`DesignSystem`](craftui_core::DesignSystem) preset (`Material3`, `Cupertino`, `WinUi`).
    pub fn set_design_preset(&mut self, preset: craftui_core::DesignSystem, mode: ThemeMode) {
        self.design_preset = preset;
        let from = self.theme_signal.get();
        let ov = self.theme_override_signal.get_untracked();
        let to = ThemeTokens::from_preset(preset, mode).applied(&ov);
        let mut spring = Spring::new(0.0, 260.0, 28.0);
        spring.set_target(1.0);
        self.theme_spring = Some((from, to, spring));
        for node in self.nodes.values_mut() {
            node.dirty |= DirtyFlags::PAINT;
        }
    }

    /// Current layout writing direction (`Ltr` or `Rtl`).
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Switch layout and text alignment direction (`Direction::Ltr` / `Direction::Rtl`), triggering re-layout.
    pub fn set_direction(&mut self, direction: Direction) {
        if self.direction == direction {
            if self.direction_signal.get_untracked() != direction {
                self.direction_signal.set(direction);
            }
            return;
        }
        self.direction = direction;
        if self.direction_signal.get_untracked() != direction {
            self.direction_signal.set(direction);
        }
        if let Some(root) = self.root {
            self.layout_engine.mark_dirty(root);
        }
        let ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in ids {
            self.layout_engine.mark_dirty(id);
            if let Some(n) = self.nodes.get_mut(id) {
                n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y;
            }
        }
    }

    /// Apply locale-appropriate layout direction (`ar`/`he`/`fa`/`ur` -> `Rtl`, else `Ltr`).
    pub fn set_locale(&mut self, locale: &str) {
        self.set_direction(craftui_core::locale_direction(locale));
    }

    pub fn set_root(&mut self, root: NodeId) {
        self.root = Some(root);
    }

    /// Return the currently focused node, if any.
    pub fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    /// Programmatically move keyboard focus to `node` (or clear it when `None`).
    pub fn set_focus(&mut self, node: Option<NodeId>, focus_visible: bool) {
        if let Some(old) = self.focused
            && self.focused != node
            && let Some(on) = self.nodes.get_mut(old)
        {
            on.dirty |= DirtyFlags::PAINT;
        }
        self.focused = node.filter(|id| self.nodes.contains_key(*id));
        self.focus_visible = focus_visible;
        if let Some(fid) = self.focused
            && let Some(nn) = self.nodes.get_mut(fid)
        {
            nn.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        }
        self.reset_caret_blink(craftui_core::monotonic_ms());
    }

    /// Register raw font bytes (`ttf`/`otf`) with the shared `TextEngine`.
    pub fn register_font_bytes(&mut self, bytes: Vec<u8>) {
        self.text_engine.register_font_bytes(bytes);
    }

    /// Register custom font bytes and append them to the sans-serif & system-ui fallback chain.
    pub fn register_fallback_font_bytes(&mut self, bytes: Vec<u8>) {
        self.text_engine.register_fallback_font_bytes(bytes);
    }

    /// Register custom monospace font bytes and append them to the `monospace` generic family.
    pub fn register_monospace_font_bytes(&mut self, bytes: Vec<u8>) {
        self.text_engine.register_monospace_font_bytes(bytes);
    }

    /// Associate a string identifier (`id: "..."`) with `node` for host lookups (e.g., `Canvas` or `PlatformView`).
    pub fn register_node_id(&mut self, node: NodeId, id: impl Into<String>) {
        if self.nodes.contains_key(node) {
            self.named_nodes.insert(id.into(), node);
        }
    }

    /// Look up a mounted node registered via `id: "..."`.
    pub fn node_by_id(&self, id: &str) -> Option<NodeId> {
        self.named_nodes
            .get(id)
            .copied()
            .filter(|nid| self.nodes.contains_key(*nid))
    }

    pub fn spawn_node(
        &mut self,
        debug_name: &'static str,
        kind: WidgetKind,
        layout: LayoutStyle,
        scope: Scope,
    ) -> NodeId {
        let id = self
            .nodes
            .insert_with_key(|id| WidgetNode::new(id, debug_name, kind, layout.clone(), scope));
        self.layout_engine.upsert_node(id, layout);
        self.init_control_anim(id);
        self.setup_reactive_bindings(id);
        id
    }

    pub(crate) fn init_control_anim(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        let anim = match &node.kind {
            WidgetKind::Button { .. } => Some(ControlVisualAnim::Button {
                hover_t: AnimatedFloat::new(0.0, TransitionCurve::tween(0.14, Easing::EaseOut)),
                press_t: AnimatedFloat::new(0.0, TransitionCurve::tween(0.08, Easing::EaseOut)),
            }),
            WidgetKind::Switch { checked, .. } => Some(ControlVisualAnim::Switch {
                knob_t: AnimatedFloat::new(
                    if checked.get_untracked() { 1.0 } else { 0.0 },
                    TransitionCurve::spring(420.0, 30.0),
                ),
            }),
            WidgetKind::Checkbox { checked, .. } => Some(ControlVisualAnim::Checkbox {
                check_t: AnimatedFloat::new(
                    if checked.get_untracked() { 1.0 } else { 0.0 },
                    TransitionCurve::spring(420.0, 26.0),
                ),
            }),
            WidgetKind::Tabs { active, .. } => Some(ControlVisualAnim::Tabs {
                active_t: AnimatedFloat::new(
                    active.get_untracked() as f32,
                    TransitionCurve::spring(360.0, 30.0),
                ),
            }),
            WidgetKind::SegmentedControl { selected, .. } => {
                Some(ControlVisualAnim::SegmentedControl {
                    active_t: AnimatedFloat::new(
                        selected.get_untracked() as f32,
                        TransitionCurve::spring(360.0, 30.0),
                    ),
                })
            }
            WidgetKind::BottomNavBar { selected, .. } => Some(ControlVisualAnim::BottomNavBar {
                active_t: AnimatedFloat::new(
                    selected.get_untracked() as f32,
                    TransitionCurve::spring(360.0, 30.0),
                ),
            }),
            WidgetKind::FloatingActionButton { .. } => {
                Some(ControlVisualAnim::FloatingActionButton {
                    hover_t: AnimatedFloat::new(0.0, TransitionCurve::spring(500.0, 36.0)),
                    press_t: AnimatedFloat::new(0.0, TransitionCurve::spring(600.0, 40.0)),
                })
            }
            WidgetKind::Slider {
                value, min, max, ..
            } => {
                let frac = if (max - min).abs() > 1e-6 {
                    ((value.get_untracked() - *min) / (*max - *min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                Some(ControlVisualAnim::Slider {
                    frac_t: AnimatedFloat::new(frac, TransitionCurve::spring(480.0, 34.0)),
                    hover_t: AnimatedFloat::new(0.0, TransitionCurve::tween(0.12, Easing::EaseOut)),
                    drag_t: AnimatedFloat::new(0.0, TransitionCurve::tween(0.12, Easing::EaseOut)),
                    dragging: false,
                })
            }
            WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. } => {
                let is_open = open.get();
                Some(ControlVisualAnim::Overlay {
                    was_open: is_open,
                    reveal: AnimatedFloat::new(
                        if is_open { 1.0 } else { 0.0 },
                        TransitionCurve::spring(320.0, 26.0),
                    ),
                })
            }
            _ => None,
        };
        if let Some(a) = anim {
            self.control_anims.insert(id, a);
        }
    }

    pub(crate) fn sync_control_anim_target(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        let Some(anim) = self.control_anims.get_mut(id) else {
            return;
        };
        match (&node.kind, anim) {
            (
                WidgetKind::Button {
                    hovered,
                    pressed,
                    disabled,
                    ..
                },
                ControlVisualAnim::Button { hover_t, press_t },
            ) => {
                let dis = node.is_disabled() || disabled.get();
                hover_t.set_target(if *hovered && !dis { 1.0 } else { 0.0 });
                press_t.set_target(if *pressed && !dis { 1.0 } else { 0.0 });
            }
            (WidgetKind::Switch { checked, .. }, ControlVisualAnim::Switch { knob_t }) => {
                knob_t.set_target(if checked.get_untracked() { 1.0 } else { 0.0 });
            }
            (WidgetKind::Checkbox { checked, .. }, ControlVisualAnim::Checkbox { check_t }) => {
                check_t.set_target(if checked.get_untracked() { 1.0 } else { 0.0 });
            }
            (WidgetKind::Tabs { tabs, active, .. }, ControlVisualAnim::Tabs { active_t }) => {
                let count = tabs.get().len();
                if count > 0 {
                    active_t.set_target(active.get_untracked().min(count - 1) as f32);
                }
            }
            (
                WidgetKind::SegmentedControl {
                    items, selected, ..
                },
                ControlVisualAnim::SegmentedControl { active_t },
            ) => {
                let count = items.get().len();
                if count > 0 {
                    active_t.set_target(selected.get_untracked().min(count - 1) as f32);
                }
            }
            (
                WidgetKind::BottomNavBar {
                    items, selected, ..
                },
                ControlVisualAnim::BottomNavBar { active_t },
            ) => {
                let count = items.get().len();
                if count > 0 {
                    active_t.set_target(selected.get_untracked().min(count - 1) as f32);
                }
            }
            (
                WidgetKind::FloatingActionButton {
                    hovered, pressed, ..
                },
                ControlVisualAnim::FloatingActionButton { hover_t, press_t },
            ) => {
                hover_t.set_target(if *hovered { 1.0 } else { 0.0 });
                press_t.set_target(if *pressed { 1.0 } else { 0.0 });
            }
            (
                WidgetKind::Slider {
                    value, min, max, ..
                },
                ControlVisualAnim::Slider {
                    frac_t, dragging, ..
                },
            ) => {
                let frac = if (max - min).abs() > 1e-6 {
                    ((value.get_untracked() - *min) / (*max - *min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                if *dragging {
                    frac_t.snap(frac);
                } else {
                    frac_t.set_target(frac);
                }
            }
            (
                WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. },
                ControlVisualAnim::Overlay { was_open, reveal },
            ) => {
                let now_open = open.get();
                if now_open && !*was_open {
                    *was_open = true;
                    reveal.snap(0.08);
                    reveal.set_target(1.0);
                } else if !now_open && *was_open {
                    *was_open = false;
                    reveal.snap(0.0);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn setup_reactive_bindings(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        let scope = node.scope;

        if let Some(bg) = node.box_style.background.clone()
            && bg.is_dynamic()
        {
            scope.effect(move || {
                let _ = bg.get();
                mark_node_dirty(id, DirtyFlags::PAINT);
            });
        }
        if let Some(bc) = node.box_style.border_color.clone()
            && bc.is_dynamic()
        {
            scope.effect(move || {
                let _ = bc.get();
                mark_node_dirty(id, DirtyFlags::PAINT);
            });
        }

        match &node.kind {
            WidgetKind::Text { content, style } => {
                let content = content.clone();
                let style = style.clone();
                if content.is_dynamic() || style.is_dynamic() {
                    scope.effect(move || {
                        let _ = content.get();
                        let _ = style.get();
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT
                                | DirtyFlags::PAINT
                                | DirtyFlags::TEXT
                                | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::RichText { content, .. } => {
                let content = content.clone();
                if content.is_dynamic() {
                    scope.effect(move || {
                        let _ = content.get();
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT
                                | DirtyFlags::PAINT
                                | DirtyFlags::TEXT
                                | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::Icon { svg, color, .. } => {
                let svg = svg.clone();
                let color_dyn = color.clone().filter(|c| c.is_dynamic());
                if svg.is_dynamic() || color_dyn.is_some() {
                    scope.effect(move || {
                        let _ = svg.get();
                        if let Some(ref c) = color_dyn {
                            let _ = c.get();
                        }
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                    });
                }
            }
            WidgetKind::Divider { color, .. } => {
                if let Some(c) = color.clone().filter(|c| c.is_dynamic()) {
                    scope.effect(move || {
                        let _ = c.get();
                        mark_node_dirty(id, DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::Button {
                label,
                disabled,
                foreground,
                ..
            } => {
                let label = label.clone();
                let disabled = disabled.clone();
                let fg_dyn = foreground.clone().filter(|f| f.is_dynamic());
                if label.is_dynamic() || disabled.is_dynamic() || fg_dyn.is_some() {
                    scope.effect(move || {
                        let _ = label.get();
                        let _ = disabled.get();
                        if let Some(ref f) = fg_dyn {
                            let _ = f.get();
                        }
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::Image { texture, .. } => {
                let texture = texture.clone();
                if texture.is_dynamic() {
                    scope.effect(move || {
                        let _ = texture.get();
                        mark_node_dirty(id, DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::VirtualList { item_count, .. } => {
                let count = item_count.clone();
                if count.is_dynamic() {
                    scope.effect(move || {
                        let _ = count.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::TextInput {
                bound_signal: Some(sig),
                ..
            } => {
                sig.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::Checkbox { checked, .. } | WidgetKind::Switch { checked, .. } => {
                checked.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::Slider { value, .. } => {
                value.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::Tabs { tabs, active, .. } => {
                active.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                let tabs = tabs.clone();
                if tabs.is_dynamic() {
                    scope.effect(move || {
                        let _ = tabs.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::Select {
                options, selected, ..
            } => {
                selected.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                let options = options.clone();
                if options.is_dynamic() {
                    scope.effect(move || {
                        let _ = options.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::RadioGroup {
                options, selected, ..
            } => {
                selected.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                let options = options.clone();
                if options.is_dynamic() {
                    scope.effect(move || {
                        let _ = options.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::PopupMenu { open, items, .. } => {
                let open = open.clone();
                let items = items.clone();
                if open.is_dynamic() || items.is_dynamic() {
                    scope.effect(move || {
                        let _ = open.get();
                        let _ = items.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::Tooltip { text } => {
                let text = text.clone();
                if text.is_dynamic() {
                    scope.effect(move || {
                        let _ = text.get();
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                    });
                }
            }
            WidgetKind::ProgressIndicator { progress, .. } => {
                let prog = progress.clone();
                if prog.is_dynamic() {
                    scope.effect(move || {
                        let _ = prog.get();
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                    });
                }
            }
            WidgetKind::Spinner { color, .. } => {
                if let Some(c) = color.clone().filter(|c| c.is_dynamic()) {
                    scope.effect(move || {
                        let _ = c.get();
                        mark_node_dirty(id, DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::Badge {
                label,
                foreground,
                background,
                icon,
            } => {
                let label = label.clone();
                let icon_dyn = icon.clone().filter(|i| i.is_dynamic());
                if label.is_dynamic() || icon_dyn.is_some() {
                    scope.effect(move || {
                        let _ = label.get();
                        if let Some(ref ic) = icon_dyn {
                            let _ = ic.get();
                        }
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
                let fg_dyn = foreground.clone().filter(|f| f.is_dynamic());
                let bg_dyn = background.clone().filter(|b| b.is_dynamic());
                if fg_dyn.is_some() || bg_dyn.is_some() {
                    scope.effect(move || {
                        if let Some(ref fg) = fg_dyn {
                            let _ = fg.get();
                        }
                        if let Some(ref bg) = bg_dyn {
                            let _ = bg.get();
                        }
                        mark_node_dirty(id, DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::SplitPane { ratio, .. } => {
                ratio.bind_node(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
            }
            WidgetKind::ToastHost { items, .. } => {
                let items = items.clone();
                if items.is_dynamic() {
                    scope.effect(move || {
                        let _ = items.get();
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                    });
                }
            }
            WidgetKind::Overlay { open, .. } => {
                let open = open.clone();
                if open.is_dynamic() {
                    scope.effect(move || {
                        let _ = open.get();
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::SegmentedControl {
                items, selected, ..
            } => {
                selected.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                let items = items.clone();
                if items.is_dynamic() {
                    scope.effect(move || {
                        let _ = items.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::BottomNavBar {
                items,
                icons,
                selected,
                ..
            } => {
                selected.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                let items = items.clone();
                let icons = icons.clone();
                if items.is_dynamic() || icons.is_dynamic() {
                    scope.effect(move || {
                        let _ = items.get();
                        let _ = icons.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::FloatingActionButton {
                icon,
                label,
                disabled,
                foreground,
                background,
                ..
            } => {
                let icon = icon.clone();
                let label = label.clone();
                let disabled = disabled.clone();
                let foreground = foreground.clone();
                let background = background.clone();
                if icon.is_dynamic()
                    || label.as_ref().map(|l| l.is_dynamic()).unwrap_or(false)
                    || disabled.is_dynamic()
                    || foreground.as_ref().map(|f| f.is_dynamic()).unwrap_or(false)
                    || background.as_ref().map(|b| b.is_dynamic()).unwrap_or(false)
                {
                    scope.effect(move || {
                        let _ = icon.get();
                        if let Some(ref l) = label {
                            let _ = l.get();
                        }
                        let _ = disabled.get();
                        if let Some(ref f) = foreground {
                            let _ = f.get();
                        }
                        if let Some(ref b) = background {
                            let _ = b.get();
                        }
                        mark_node_dirty(
                            id,
                            DirtyFlags::PAINT | DirtyFlags::LAYOUT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::HtmlView { src, html, .. } => {
                let src = src.clone();
                let html = html.clone();
                if src.is_dynamic() || html.is_dynamic() {
                    scope.effect(move || {
                        let _ = src.get();
                        let _ = html.get();
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::LAYOUT);
                    });
                }
            }
            WidgetKind::Accordion {
                title, expanded, ..
            } => {
                expanded.bind_node(
                    id,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                );
                let title = title.clone();
                if title.is_dynamic() {
                    scope.effect(move || {
                        let _ = title.get();
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::DataTable {
                columns,
                rows,
                selected,
                ..
            } => {
                selected.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                let columns = columns.clone();
                let rows = rows.clone();
                if columns.is_dynamic() || rows.is_dynamic() {
                    scope.effect(move || {
                        let _ = columns.get();
                        let _ = rows.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::ListTile {
                title,
                subtitle,
                leading,
                trailing,
                selected,
                ..
            } => {
                let selected = selected.clone();
                let title = title.clone();
                let subtitle = subtitle.clone();
                let leading = leading.clone();
                let trailing = trailing.clone();
                if selected.is_dynamic()
                    || title.is_dynamic()
                    || subtitle.as_ref().is_some_and(|s| s.is_dynamic())
                    || leading.as_ref().is_some_and(|l| l.is_dynamic())
                    || trailing.as_ref().is_some_and(|t| t.is_dynamic())
                {
                    scope.effect(move || {
                        let _ = selected.get();
                        let _ = title.get();
                        if let Some(ref s) = subtitle {
                            let _ = s.get();
                        }
                        if let Some(ref l) = leading {
                            let _ = l.get();
                        }
                        if let Some(ref t) = trailing {
                            let _ = t.get();
                        }
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::Chip {
                label,
                icon,
                selected,
                ..
            } => {
                selected.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                let label = label.clone();
                let icon = icon.clone();
                if label.is_dynamic() || icon.as_ref().is_some_and(|i| i.is_dynamic()) {
                    scope.effect(move || {
                        let _ = label.get();
                        if let Some(ref i) = icon {
                            let _ = i.get();
                        }
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::AppBar {
                title,
                leading_icon,
                ..
            } => {
                let title = title.clone();
                let leading_icon = leading_icon.clone();
                if title.is_dynamic() || leading_icon.as_ref().is_some_and(|i| i.is_dynamic()) {
                    scope.effect(move || {
                        let _ = title.get();
                        if let Some(ref i) = leading_icon {
                            let _ = i.get();
                        }
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                    });
                }
            }
            WidgetKind::Drawer { open, .. } => {
                open.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::DatePicker { value, .. } | WidgetKind::TimePicker { value, .. } => {
                value.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::TreeView {
                items, selected, ..
            } => {
                selected.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                let items = items.clone();
                if items.is_dynamic() {
                    scope.effect(move || {
                        let _ = items.get();
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::ReorderableList {
                items, selected, ..
            } => {
                items.bind_node(
                    id,
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                );
                selected.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::MenuBar { menus, .. } => {
                let menus = menus.clone();
                if menus.is_dynamic() {
                    scope.effect(move || {
                        let _ = menus.get();
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::Draggable { payload, .. } => {
                let payload = payload.clone();
                if payload.is_dynamic() {
                    scope.effect(move || {
                        let _ = payload.get();
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                    });
                }
            }
            WidgetKind::MediaPlayer {
                source, controller, ..
            } => {
                let source = source.clone();
                if source.is_dynamic() {
                    scope.effect(move || {
                        let _ = source.get();
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                    });
                }
                controller.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::RichTextEditor { controller, .. } => {
                controller.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y | DirtyFlags::TEXT);
            }
            _ => {}
        }

        // Theme updates re-paint widgets without full layout recalculation (Design Spec §4).
        self.theme_signal.bind_node(id, DirtyFlags::PAINT);
        self.theme_override_signal.bind_node(id, DirtyFlags::PAINT);
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        if let Some(c) = self.nodes.get_mut(child) {
            c.parent = Some(parent);
        }
        if let Some(p) = self.nodes.get_mut(parent) {
            p.children.push(child);
            p.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            let children = p.children.clone();
            self.layout_engine.set_children(parent, &children);
        }
    }

    pub fn set_children(&mut self, parent: NodeId, children: Vec<NodeId>) {
        for &c in &children {
            if let Some(cn) = self.nodes.get_mut(c) {
                cn.parent = Some(parent);
            }
        }
        if let Some(p) = self.nodes.get_mut(parent) {
            p.children = children.clone();
            p.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
        }
        self.layout_engine.set_children(parent, &children);
    }

    /// Remove a subtree cleanly and dispose its reactive scope.
    pub fn remove_subtree(&mut self, node_id: NodeId) {
        if let Some(parent) = self.nodes.get(node_id).and_then(|n| n.parent)
            && let Some(p) = self.nodes.get_mut(parent)
        {
            p.children.retain(|&c| c != node_id);
            let children = p.children.clone();
            self.layout_engine.set_children(parent, &children);
        }
        self.remove_subtree_inner(node_id);
    }

    pub(crate) fn remove_subtree_inner(&mut self, node_id: NodeId) {
        let Some(node) = self.nodes.remove(node_id) else {
            return;
        };
        for child in node.children {
            self.remove_subtree_inner(child);
        }
        self.virtual_hosts.remove(node_id);
        self.dynamic_hosts.remove(node_id);
        self.dynamic_prop_updaters.remove(node_id);
        self.node_anims.remove(node_id);
        self.control_anims.remove(node_id);
        self.named_nodes.retain(|_, v| *v != node_id);
        if let Some(mut scopes) = self.prop_scopes.remove(node_id) {
            for s in [
                scopes.box_bg.take(),
                scopes.box_border_color.take(),
                scopes.divider_color.take(),
                scopes.button_fg.take(),
                scopes.spinner_color.take(),
                scopes.badge_fg.take(),
                scopes.badge_bg.take(),
                scopes.badge_icon.take(),
                scopes.disabled.take(),
                scopes.visible_when.take(),
            ]
            .into_iter()
            .flatten()
            .chain(scopes.dynamic_props.drain(..))
            {
                s.dispose();
            }
        }
        self.layout_engine.remove_node(node_id);
        if node.scope != self.root_scope {
            node.scope.dispose();
        }
        if self.focused == Some(node_id) {
            self.focused = None;
            self.caret_next_toggle_ms = None;
            self.last_caret_focused = None;
            self.caret_visible = true;
        }
        if self.hovered == Some(node_id) {
            self.hovered = None;
        }
    }

    pub(crate) fn dispose_prop_effect(
        &mut self,
        node: NodeId,
        slot: fn(&mut PropEffectScopes) -> &mut Option<Scope>,
    ) {
        if let Some(entry) = self.prop_scopes.get_mut(node)
            && let Some(old) = slot(entry).take()
        {
            old.dispose();
        }
    }

    pub(crate) fn reset_prop_effect(
        &mut self,
        node: NodeId,
        slot: fn(&mut PropEffectScopes) -> &mut Option<Scope>,
    ) -> Option<Scope> {
        self.dispose_prop_effect(node, slot);
        let parent_scope = self.nodes.get(node)?.scope;
        if !self.prop_scopes.contains_key(node) {
            self.prop_scopes.insert(node, PropEffectScopes::default());
        }
        if let Some(scopes) = self.prop_scopes.get_mut(node) {
            let child = parent_scope.child_scope();
            *slot(scopes) = Some(child);
            Some(child)
        } else {
            None
        }
    }

    pub fn focused_is_text_input(&self) -> bool {
        self.focused
            .and_then(|id| self.nodes.get(id))
            .is_some_and(|n| {
                !n.is_disabled()
                    && matches!(
                        n.kind,
                        WidgetKind::TextInput { .. } | WidgetKind::RichTextEditor { .. }
                    )
            })
    }

    pub fn reset_caret_blink(&mut self, now_ms: u64) {
        if self.focused_is_text_input() {
            self.caret_visible = true;
            self.caret_next_toggle_ms = Some(now_ms.saturating_add(CARET_BLINK_INTERVAL_MS));
            self.last_caret_focused = self.focused;
        } else {
            self.caret_visible = true;
            self.caret_next_toggle_ms = None;
            self.last_caret_focused = None;
        }
    }

    pub(crate) fn ensure_caret_blink_armed(&mut self, now_ms: u64) {
        if self.focused_is_text_input() {
            if self.last_caret_focused != self.focused || self.caret_next_toggle_ms.is_none() {
                self.reset_caret_blink(now_ms);
            }
        } else {
            self.caret_visible = true;
            self.caret_next_toggle_ms = None;
            self.last_caret_focused = None;
        }
    }

    /// Advance demand-driven cursor caret blink timers at `now_ms`.
    /// Returns `true` when visibility toggled and a repaint is required.
    pub fn tick_blink_timers(&mut self, now_ms: u64) -> bool {
        self.ensure_caret_blink_armed(now_ms);
        if let Some(next_ms) = self.caret_next_toggle_ms
            && now_ms >= next_ms
        {
            self.caret_visible = !self.caret_visible;
            self.caret_next_toggle_ms = Some(now_ms.saturating_add(CARET_BLINK_INTERVAL_MS));
            if let Some(fid) = self.focused
                && let Some(n) = self.nodes.get_mut(fid)
            {
                n.dirty |= DirtyFlags::PAINT;
            }
            return true;
        }
        false
    }

    /// Next wake delay (in ms) required by internal blink timers when idle, if any.
    pub fn next_internal_delay_ms(&self, now_ms: u64) -> Option<u64> {
        if self.focused_is_text_input() {
            self.caret_next_toggle_ms
                .map(|due| due.saturating_sub(now_ms))
        } else {
            None
        }
    }

    pub(crate) fn node_visual_rect(&self, id: NodeId) -> Rect {
        let mut off = Point::ZERO;
        let mut cur = self.nodes.get(id).and_then(|n| n.parent);
        while let Some(pid) = cur {
            if let Some(pn) = self.nodes.get(pid) {
                if let WidgetKind::ScrollView { axis, offset, .. } = &pn.kind {
                    match axis {
                        Axis::Vertical => off.y -= *offset,
                        Axis::Horizontal => off.x -= *offset,
                    }
                }
                cur = pn.parent;
            } else {
                break;
            }
        }
        self.layout_engine.rect(id).translate(off.x, off.y)
    }
}

pub mod a11y;
pub mod anim;
pub mod builders;
pub mod events;
pub mod layout;
pub mod modifiers;
pub mod paint;
pub mod reactive;
pub mod state;
