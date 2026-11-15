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
use craftui_text::{TextEditorState, TextEngine, TextStyle};
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
    fn new(
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
struct PropEffectScopes {
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
    fn is_animating(&self) -> bool {
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

    fn step(&mut self, dt_sec: f32) -> (bool, bool) {
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
    fn is_animating(&self) -> bool {
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

    fn step(&mut self, dt_sec: f32) -> bool {
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
    pub metrics_signal: Signal<WindowMetrics>,
    pub services: PlatformServices,
    pub direction: Direction,
    pub focused: Option<NodeId>,
    pub focus_visible: bool,
    pub hovered: Option<NodeId>,
    last_pointer_pos: Option<Point>,
    pub metrics: WindowMetrics,
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
        root_scope.provide_context(theme_signal);
        root_scope.provide_context(theme_override_signal);
        root_scope.provide_context(metrics_signal);
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
            metrics_signal,
            services,
            direction: Direction::Ltr,
            focused: None,
            focus_visible: false,
            hovered: None,
            last_pointer_pos: None,
            metrics: WindowMetrics::default(),
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
        let base = ThemeTokens::from_mode(self.theme_signal.get_untracked().mode);
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
        let to = ThemeTokens::from_mode(mode).applied(&ov);
        let mut spring = Spring::new(0.0, 260.0, 28.0);
        spring.set_target(1.0);
        self.theme_spring = Some((from, to, spring));
    }

    pub fn set_root(&mut self, root: NodeId) {
        self.root = Some(root);
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

    fn init_control_anim(&mut self, id: NodeId) {
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

    fn sync_control_anim_target(&mut self, id: NodeId) {
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

    fn setup_reactive_bindings(&mut self, id: NodeId) {
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

    fn remove_subtree_inner(&mut self, node_id: NodeId) {
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

    fn dispose_prop_effect(
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

    fn reset_prop_effect(
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
            .is_some_and(|n| !n.is_disabled() && matches!(n.kind, WidgetKind::TextInput { .. }))
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

    fn ensure_caret_blink_armed(&mut self, now_ms: u64) {
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

    fn node_visual_rect(&self, id: NodeId) -> Rect {
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

    // === Ergonomic Widget Builders ===

    pub fn container(&mut self, scope: Scope, layout: LayoutStyle, style: BoxStyle) -> NodeId {
        let id = self.spawn_node("Container", WidgetKind::Container, layout, scope);
        self.set_box_style(id, style);
        id
    }

    pub fn row(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::row(), BoxStyle::new())
    }

    pub fn column(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::column(), BoxStyle::new())
    }

    pub fn grid(&mut self, scope: Scope, columns: u16, gap: f32) -> NodeId {
        self.container(
            scope,
            LayoutStyle::grid(columns).with_gap(gap),
            BoxStyle::new(),
        )
    }

    pub fn stack(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::stack(), BoxStyle::new())
    }

    pub fn spacer(&mut self, scope: Scope) -> NodeId {
        self.spawn_node(
            "Spacer",
            WidgetKind::Spacer,
            LayoutStyle::column().with_flex_grow(1.0),
            scope,
        )
    }

    pub fn safe_area(&mut self, scope: Scope) -> NodeId {
        self.spawn_node(
            "SafeArea",
            WidgetKind::SafeArea,
            LayoutStyle::column()
                .with_size(Length::Pct(100.0), Length::Pct(100.0))
                .with_safe_area(true),
            scope,
        )
    }

    pub fn divider(&mut self, scope: Scope, axis: Axis, thickness: f32) -> NodeId {
        let th = thickness.max(1.0);
        let mut layout = match axis {
            Axis::Horizontal => LayoutStyle::row().with_height(Length::Dp(th)),
            Axis::Vertical => LayoutStyle::column().with_width(Length::Dp(th)),
        };
        if matches!(axis, Axis::Horizontal) {
            layout.width = Length::Pct(100.0);
        } else {
            layout.height = Length::Pct(100.0);
        }
        let id = self.spawn_node(
            "Divider",
            WidgetKind::Divider {
                axis,
                thickness: th,
                color: None,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Divider;
        }
        id
    }

    pub fn set_divider_color(&mut self, node: NodeId, color: impl Into<ReactiveValue<Color>>) {
        let c = color.into();
        let is_div = self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Divider { .. }));
        if !is_div {
            return;
        }
        if c.is_dynamic() {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.divider_color) {
                let cc = c.clone();
                eff_scope.effect(move || {
                    let _ = cc.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.divider_color);
        }
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Divider { color: slot, .. } = &mut n.kind
        {
            *slot = Some(c);
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn text(
        &mut self,
        scope: Scope,
        content: impl Into<ReactiveValue<String>>,
        style: impl Into<ReactiveValue<TextStyle>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "Text",
            WidgetKind::Text {
                content: content.into(),
                style: style.into(),
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Label;
        }
        id
    }

    pub fn icon(
        &mut self,
        scope: Scope,
        svg: impl Into<ReactiveValue<String>>,
        size: f32,
        color: Option<ReactiveValue<Color>>,
    ) -> NodeId {
        let sz = size.max(8.0);
        let svg_rv = svg.into();
        let label = svg_rv.get();
        let id = self.spawn_node(
            "Icon",
            WidgetKind::Icon {
                svg: svg_rv,
                size: sz,
                color,
            },
            LayoutStyle::row().with_size(Length::Dp(sz), Length::Dp(sz)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Image;
            n.a11y.label = Some(if label.starts_with('<') {
                "Icon".to_string()
            } else {
                label
            });
        }
        id
    }

    pub fn button(
        &mut self,
        scope: Scope,
        label: impl Into<ReactiveValue<String>>,
        on_tap: impl FnMut() + 'static,
    ) -> NodeId {
        let label = label.into();
        let label_text = label.get();
        let id = self.spawn_node(
            "Button",
            WidgetKind::Button {
                label,
                disabled: ReactiveValue::Static(false),
                foreground: None,
                on_tap: Some(Rc::new(RefCell::new(on_tap))),
                hovered: false,
                pressed: false,
            },
            LayoutStyle::row()
                .with_padding(Insets::symmetric(18.0, 10.0))
                .with_align(Alignment::Center),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(10.0);
            n.a11y.role = A11yRole::Button;
            n.a11y.label = Some(label_text);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn image(
        &mut self,
        scope: Scope,
        texture: impl Into<ReactiveValue<Option<TextureId>>>,
        fit: Fit,
        radius: CornerRadii,
    ) -> NodeId {
        let id = self.spawn_node(
            "ImageView",
            WidgetKind::Image {
                texture: texture.into(),
                fit,
                radius,
                opacity: 1.0,
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Image;
        }
        id
    }

    pub fn canvas(
        &mut self,
        scope: Scope,
        layout: LayoutStyle,
        on_paint: impl Fn(Rect, &mut PaintScene) + 'static,
        on_pointer: Option<CanvasPointerFn>,
    ) -> NodeId {
        let id = self.spawn_node(
            "Canvas",
            WidgetKind::Canvas {
                on_paint: Rc::new(on_paint),
                on_pointer,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Canvas;
        }
        id
    }

    pub fn scroll_view(&mut self, scope: Scope, axis: Axis, mut layout: LayoutStyle) -> NodeId {
        layout.clip_overflow = true;
        let id = self.spawn_node(
            "ScrollView",
            WidgetKind::ScrollView {
                axis,
                offset: 0.0,
                velocity: 0.0,
                content_extent: 0.0,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.clip = true;
            n.a11y.role = A11yRole::ScrollView;
        }
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn virtual_list(
        &mut self,
        scope: Scope,
        axis: Axis,
        columns: usize,
        item_extent: f32,
        item_count: impl Into<ReactiveValue<usize>>,
        mut layout: LayoutStyle,
        factory: VirtualItemFactory,
    ) -> NodeId {
        layout.clip_overflow = true;
        let id = self.spawn_node(
            "VirtualList",
            WidgetKind::VirtualList {
                axis,
                columns: columns.max(1),
                item_extent: item_extent.max(8.0),
                item_count: item_count.into(),
                offset: 0.0,
                velocity: 0.0,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.clip = true;
            n.a11y.role = A11yRole::List;
        }
        self.virtual_hosts.insert(
            id,
            VirtualListHost {
                factory,
                active_items: HashMap::new(),
                last_range: None,
            },
        );
        id
    }

    pub fn text_input(
        &mut self,
        scope: Scope,
        initial: impl Into<String>,
        placeholder: impl Into<String>,
        bound_signal: Option<Signal<String>>,
    ) -> NodeId {
        let initial_s = bound_signal
            .map(|s| s.get_untracked())
            .unwrap_or_else(|| initial.into());
        let placeholder = placeholder.into();
        let accessible_name = (!placeholder.is_empty()).then(|| placeholder.clone());
        let id = self.spawn_node(
            "TextInput",
            WidgetKind::TextInput {
                editor: TextEditorState::new(initial_s.clone()),
                placeholder,
                style: TextStyle::default().with_size(14.0),
                multiline: false,
                bound_signal,
                on_change: None,
                on_submit: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(40.0))
                .with_padding(Insets::symmetric(12.0, 8.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::TextInput;
            // Without a name a screen reader only announces "edit text".
            n.a11y.label = accessible_name;
            n.a11y.value = Some(initial_s);
            n.a11y.focusable = true;
        }
        id
    }

    /// Multi-line editable text area widget with wrapping, newline Enter, and Up/Down line navigation.
    pub fn text_area(
        &mut self,
        scope: Scope,
        initial: impl Into<String>,
        placeholder: impl Into<String>,
        bound_signal: Option<Signal<String>>,
    ) -> NodeId {
        let initial_s = bound_signal
            .map(|s| s.get_untracked())
            .unwrap_or_else(|| initial.into());
        let placeholder = placeholder.into();
        let accessible_name = (!placeholder.is_empty()).then(|| placeholder.clone());
        let id = self.spawn_node(
            "TextArea",
            WidgetKind::TextInput {
                editor: TextEditorState::new(initial_s.clone()),
                placeholder,
                style: TextStyle::default().with_size(14.0),
                multiline: true,
                bound_signal,
                on_change: None,
                on_submit: None,
            },
            LayoutStyle::column()
                .with_height(Length::Dp(104.0))
                .with_padding(Insets::symmetric(12.0, 10.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::MultilineTextInput;
            // Without a name a screen reader only announces "edit text".
            n.a11y.label = accessible_name;
            n.a11y.value = Some(initial_s);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn checkbox(
        &mut self,
        scope: Scope,
        checked: Signal<bool>,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let id = self.spawn_node(
            "Checkbox",
            WidgetKind::Checkbox {
                checked,
                label: lbl.clone(),
                on_change: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(32.0))
                .with_gap(8.0),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::CheckBox;
            n.a11y.label = Some(lbl);
            n.a11y.checked = Some(checked.get_untracked());
            n.a11y.focusable = true;
        }
        id
    }

    pub fn switch(
        &mut self,
        scope: Scope,
        checked: Signal<bool>,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let id = self.spawn_node(
            "Switch",
            WidgetKind::Switch {
                checked,
                label: lbl.clone(),
                on_change: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(32.0))
                .with_gap(10.0),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Switch;
            n.a11y.label = Some(lbl);
            n.a11y.checked = Some(checked.get_untracked());
            n.a11y.focusable = true;
        }
        id
    }

    pub fn slider(
        &mut self,
        scope: Scope,
        value: Signal<f32>,
        min: f32,
        max: f32,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let default_h = if lbl.is_empty() { 32.0 } else { 48.0 };
        let id = self.spawn_node(
            "Slider",
            WidgetKind::Slider {
                value,
                min,
                max,
                label: lbl.clone(),
                on_change: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(default_h))
                .with_padding(Insets::symmetric(4.0, 0.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.hit_slop = 10.0;
            n.a11y.role = A11yRole::Slider;
            n.a11y.label = Some(lbl);
            n.a11y.numeric_value = Some(value.get_untracked() as f64);
            n.a11y.min_value = Some(min as f64);
            n.a11y.max_value = Some(max as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn tabs(
        &mut self,
        scope: Scope,
        tabs: impl Into<ReactiveValue<Vec<String>>>,
        active: Signal<usize>,
    ) -> NodeId {
        let tabs_rv = tabs.into();
        let id = self.spawn_node(
            "Tabs",
            WidgetKind::Tabs {
                tabs: tabs_rv,
                active,
                on_change: None,
            },
            LayoutStyle::row().with_height(Length::Dp(40.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::TabList;
            n.a11y.numeric_value = Some(active.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn select(
        &mut self,
        scope: Scope,
        options: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
        placeholder: impl Into<String>,
    ) -> NodeId {
        let opts_rv = options.into();
        let opts = opts_rv.get();
        let cur = opts.get(selected.get_untracked()).cloned();
        let id = self.spawn_node(
            "Select",
            WidgetKind::Select {
                options: opts_rv,
                selected,
                open: false,
                dropdown_scroll: 0.0,
                placeholder: placeholder.into(),
                on_change: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(40.0))
                .with_padding(Insets::symmetric(12.0, 8.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::ComboBox;
            n.a11y.value = cur;
            n.a11y.focusable = true;
        }
        id
    }

    pub fn radio_group(
        &mut self,
        scope: Scope,
        options: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
        axis: Axis,
    ) -> NodeId {
        let id = self.spawn_node(
            "RadioGroup",
            WidgetKind::RadioGroup {
                options: options.into(),
                selected,
                axis,
                on_change: None,
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::RadioButton;
            n.a11y.numeric_value = Some(selected.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn segmented_control(
        &mut self,
        scope: Scope,
        items: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
    ) -> NodeId {
        let id = self.spawn_node(
            "SegmentedControl",
            WidgetKind::SegmentedControl {
                items: items.into(),
                selected,
                on_change: None,
            },
            LayoutStyle::row().with_height(Length::Dp(36.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(10.0);
            n.a11y.role = A11yRole::TabList;
            n.a11y.numeric_value = Some(selected.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn bottom_nav_bar(
        &mut self,
        scope: Scope,
        items: impl Into<ReactiveValue<Vec<String>>>,
        icons: impl Into<ReactiveValue<Vec<String>>>,
        selected: Signal<usize>,
    ) -> NodeId {
        let id = self.spawn_node(
            "BottomNavBar",
            WidgetKind::BottomNavBar {
                items: items.into(),
                icons: icons.into(),
                selected,
                on_change: None,
            },
            LayoutStyle::row().with_height(Length::Dp(64.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::TabList;
            n.a11y.numeric_value = Some(selected.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn floating_action_button(
        &mut self,
        scope: Scope,
        icon: impl Into<ReactiveValue<String>>,
        label: Option<impl Into<ReactiveValue<String>>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "FloatingActionButton",
            WidgetKind::FloatingActionButton {
                icon: icon.into(),
                label: label.map(|l| l.into()),
                disabled: false.into(),
                foreground: None,
                background: None,
                on_tap: None,
                hovered: false,
                pressed: false,
            },
            LayoutStyle::row().with_size(Length::Dp(56.0), Length::Dp(56.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Button;
            n.a11y.focusable = true;
        }
        id
    }

    pub fn accordion(
        &mut self,
        scope: Scope,
        title: impl Into<ReactiveValue<String>>,
        expanded: Signal<bool>,
    ) -> NodeId {
        let title_rv = title.into();
        let lbl = title_rv.get();
        let is_exp = expanded.get_untracked();
        let mut layout = LayoutStyle::column();
        layout.header_top_inset = 44.0;
        let id = self.spawn_node(
            "Accordion",
            WidgetKind::Accordion {
                title: title_rv,
                expanded,
                on_change: None,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(10.0);
            n.a11y.role = A11yRole::Button;
            n.a11y.label = Some(lbl);
            n.a11y.checked = Some(is_exp);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn data_table(
        &mut self,
        scope: Scope,
        columns: impl Into<ReactiveValue<Vec<String>>>,
        rows: impl Into<ReactiveValue<Vec<Vec<String>>>>,
        selected: Signal<usize>,
    ) -> NodeId {
        let id = self.spawn_node(
            "DataTable",
            WidgetKind::DataTable {
                columns: columns.into(),
                rows: rows.into(),
                selected,
                sort_column: None,
                sort_ascending: true,
                scroll_offset: 0.0,
                striped: true,
                row_height: 36.0,
                on_select: None,
                on_sort: None,
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.box_style.clip = true;
            n.a11y.role = A11yRole::List;
            n.a11y.numeric_value = Some(selected.get_untracked() as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn set_data_table_options(
        &mut self,
        node: NodeId,
        striped_opt: Option<bool>,
        row_height_opt: Option<f32>,
    ) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::DataTable {
                striped,
                row_height,
                ..
            } = &mut n.kind
        {
            if let Some(s) = striped_opt {
                *striped = s;
            }
            if let Some(rh) = row_height_opt {
                *row_height = rh.clamp(24.0, 96.0);
                n.dirty |= DirtyFlags::LAYOUT;
                self.layout_engine.mark_dirty(node);
            }
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_on_sort(&mut self, node: NodeId, cb: impl FnMut(usize) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::DataTable { on_sort, .. } = &mut n.kind
        {
            *on_sort = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn popup_menu(
        &mut self,
        scope: Scope,
        open: impl Into<ReactiveValue<bool>>,
        items: impl Into<ReactiveValue<Vec<String>>>,
        anchor: Option<Point>,
        on_select: Option<IndexCallback>,
        on_close: Option<TapHandler>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        let id = self.spawn_node(
            "PopupMenu",
            WidgetKind::PopupMenu {
                open: open.into(),
                items: items.into(),
                anchor,
                on_select,
                on_close,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Menu;
        }
        id
    }

    pub fn tooltip(&mut self, scope: Scope, text: impl Into<ReactiveValue<String>>) -> NodeId {
        let rv = text.into();
        let lbl = rv.get();
        let id = self.spawn_node(
            "Tooltip",
            WidgetKind::Tooltip { text: rv },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Tooltip;
            n.a11y.label = Some(lbl);
        }
        id
    }

    pub fn progress_indicator(
        &mut self,
        scope: Scope,
        progress: impl Into<ReactiveValue<Option<f32>>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "ProgressIndicator",
            WidgetKind::ProgressIndicator {
                progress: progress.into(),
                phase: 0.0,
            },
            LayoutStyle::row().with_height(Length::Dp(8.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::ProgressIndicator;
        }
        id
    }

    pub fn spinner(&mut self, scope: Scope, size: f32) -> NodeId {
        let sz = size.max(12.0);
        let id = self.spawn_node(
            "Spinner",
            WidgetKind::Spinner {
                size: sz,
                thickness: (sz * 0.12).clamp(2.0, 6.0),
                color: None,
                phase: 0.0,
            },
            LayoutStyle::row().with_size(Length::Dp(sz), Length::Dp(sz)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::ProgressIndicator;
            n.a11y.label = Some("Loading".to_string());
        }
        id
    }

    pub fn set_spinner_style(
        &mut self,
        node: NodeId,
        size: Option<f32>,
        thickness: Option<f32>,
        color: Option<ReactiveValue<Color>>,
    ) {
        if !self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Spinner { .. }))
        {
            return;
        }
        if let Some(c) = color.clone() {
            if c.is_dynamic() {
                if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.spinner_color) {
                    let cc = c.clone();
                    eff_scope.effect(move || {
                        let _ = cc.get();
                        mark_node_dirty(node, DirtyFlags::PAINT);
                    });
                }
            } else {
                self.dispose_prop_effect(node, |s| &mut s.spinner_color);
            }
        }
        let mut updated_layout = None;
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Spinner {
                size: sz_slot,
                thickness: th_slot,
                color: col_slot,
                ..
            } = &mut n.kind
        {
            if let Some(sz) = size {
                let next_sz = sz.max(12.0);
                *sz_slot = next_sz;
                n.layout.width = Length::Dp(next_sz);
                n.layout.height = Length::Dp(next_sz);
                updated_layout = Some(n.layout.clone());
                n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            }
            if let Some(th) = thickness {
                *th_slot = th.max(1.5);
                n.dirty |= DirtyFlags::PAINT;
            }
            if let Some(c) = color {
                *col_slot = Some(c);
                n.dirty |= DirtyFlags::PAINT;
            }
        }
        if let Some(st) = updated_layout {
            self.layout_engine.upsert_node(node, st);
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn badge(&mut self, scope: Scope, label: impl Into<ReactiveValue<String>>) -> NodeId {
        let rv = label.into();
        let text = rv.get();
        let id = self.spawn_node(
            "Badge",
            WidgetKind::Badge {
                label: rv,
                foreground: None,
                background: None,
                icon: None,
            },
            LayoutStyle::row()
                .with_padding(Insets::symmetric(10.0, 4.0))
                .with_align(Alignment::Center),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(12.0);
            n.a11y.role = A11yRole::Label;
            n.a11y.label = Some(text);
        }
        id
    }

    pub fn set_badge_colors(
        &mut self,
        node: NodeId,
        foreground: Option<ReactiveValue<Color>>,
        background: Option<ReactiveValue<Color>>,
    ) {
        if !self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Badge { .. }))
        {
            return;
        }
        if let Some(fg) = foreground.clone() {
            if fg.is_dynamic() {
                if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.badge_fg) {
                    let fg_c = fg.clone();
                    eff_scope.effect(move || {
                        let _ = fg_c.get();
                        mark_node_dirty(node, DirtyFlags::PAINT);
                    });
                }
            } else {
                self.dispose_prop_effect(node, |s| &mut s.badge_fg);
            }
        }
        if let Some(bg) = background.clone() {
            if bg.is_dynamic() {
                if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.badge_bg) {
                    let bg_c = bg.clone();
                    eff_scope.effect(move || {
                        let _ = bg_c.get();
                        mark_node_dirty(node, DirtyFlags::PAINT);
                    });
                }
            } else {
                self.dispose_prop_effect(node, |s| &mut s.badge_bg);
            }
        }
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Badge {
                foreground: fg_slot,
                background: bg_slot,
                ..
            } = &mut n.kind
        {
            if foreground.is_some() {
                *fg_slot = foreground;
            }
            if background.is_some() {
                *bg_slot = background;
            }
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_badge_icon(&mut self, node: NodeId, icon: Option<ReactiveValue<String>>) {
        if !self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Badge { .. }))
        {
            return;
        }
        if let Some(ic) = icon.clone() {
            if ic.is_dynamic() {
                if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.badge_icon) {
                    let ic_c = ic.clone();
                    eff_scope.effect(move || {
                        let _ = ic_c.get();
                        mark_node_dirty(node, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            } else {
                self.dispose_prop_effect(node, |s| &mut s.badge_icon);
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.badge_icon);
        }
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Badge { icon: slot, .. } = &mut n.kind
        {
            *slot = icon;
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn split_pane(&mut self, scope: Scope, axis: Axis, ratio: Signal<f32>) -> NodeId {
        let div = 8.0;
        let mut layout = match axis {
            Axis::Horizontal => LayoutStyle::row(),
            Axis::Vertical => LayoutStyle::column(),
        };
        layout.align_items = Alignment::Stretch;
        layout.width = Length::Pct(100.0);
        layout.height = Length::Pct(100.0);
        layout.clip_overflow = true;
        match axis {
            Axis::Horizontal => layout.gap_x = div,
            Axis::Vertical => layout.gap_y = div,
        }
        let id = self.spawn_node(
            "SplitPane",
            WidgetKind::SplitPane {
                axis,
                ratio,
                min_first: 80.0,
                min_second: 80.0,
                divider_size: div,
                dragging: false,
                on_change: None,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.clip = true;
            n.a11y.role = A11yRole::Container;
        }
        id
    }

    pub fn set_split_pane_config(
        &mut self,
        node: NodeId,
        min_first: f32,
        min_second: f32,
        divider_size: f32,
    ) {
        let mut updated_layout = None;
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::SplitPane {
                axis,
                min_first: mf,
                min_second: ms,
                divider_size: ds,
                ..
            } = &mut n.kind
        {
            *mf = min_first.max(0.0);
            *ms = min_second.max(0.0);
            *ds = divider_size.max(4.0);
            match axis {
                Axis::Horizontal => n.layout.gap_x = *ds,
                Axis::Vertical => n.layout.gap_y = *ds,
            }
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            updated_layout = Some(n.layout.clone());
        }
        if let Some(st) = updated_layout {
            self.layout_engine.upsert_node(node, st);
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn toast_host(&mut self, scope: Scope, manager: ToastManager) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        let id = self.spawn_node(
            "ToastHost",
            WidgetKind::ToastHost {
                items: ReactiveValue::from(manager.items_signal()),
                on_dismiss: None,
                manager: Some(manager),
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Alert;
            n.a11y.label = Some("Notifications".to_string());
        }
        id
    }

    pub fn toast_host_custom(
        &mut self,
        scope: Scope,
        items: impl Into<ReactiveValue<Vec<ToastItem>>>,
        on_dismiss: Option<ToastDismissCallback>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        let id = self.spawn_node(
            "ToastHost",
            WidgetKind::ToastHost {
                items: items.into(),
                on_dismiss,
                manager: None,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Alert;
            n.a11y.label = Some("Notifications".to_string());
        }
        id
    }

    pub fn set_on_dismiss(&mut self, node: NodeId, cb: impl FnMut(u64) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::ToastHost { on_dismiss, .. } = &mut n.kind
        {
            *on_dismiss = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn overlay(
        &mut self,
        scope: Scope,
        open: impl Into<ReactiveValue<bool>>,
        is_bottom_sheet: bool,
        on_close: Option<TapHandler>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        layout.justify_content = if is_bottom_sheet {
            craftui_core::Justify::End
        } else {
            craftui_core::Justify::Center
        };
        layout.align_items = Alignment::Center;
        let id = self.spawn_node(
            if is_bottom_sheet {
                "BottomSheet"
            } else {
                "Modal"
            },
            WidgetKind::Overlay {
                open: open.into(),
                on_close,
                is_bottom_sheet,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Dialog;
        }
        id
    }

    pub fn platform_view(
        &mut self,
        scope: Scope,
        view_id: PlatformViewId,
        layout: LayoutStyle,
    ) -> NodeId {
        self.spawn_node(
            "PlatformView",
            WidgetKind::PlatformView { id: view_id },
            layout,
            scope,
        )
    }

    pub fn html_view(
        &mut self,
        scope: Scope,
        src: impl Into<ReactiveValue<String>>,
        html: impl Into<ReactiveValue<String>>,
        router: Option<Rc<RefCell<craftui_core::IpcRouter>>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "HtmlView",
            WidgetKind::HtmlView {
                src: src.into(),
                html: html.into(),
                router,
            },
            LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Container;
        }
        id
    }

    /// Mount a Material 3 Card panel container matching Flutter's `Card` (`Elevated`, `Filled`, `Outlined`).
    pub fn card(&mut self, scope: Scope, variant: CardVariant) -> NodeId {
        let layout = LayoutStyle::column().with_padding(Insets::all(16.0));
        let box_style = BoxStyle {
            radius: CornerRadii::all(12.0),
            ..BoxStyle::default()
        };
        let id = self.spawn_node("Card", WidgetKind::Card { variant }, layout, scope);
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style = box_style;
            n.a11y.role = A11yRole::Container;
        }
        id
    }

    /// Mount a Material 3 item row matching Flutter's `ListTile`.
    pub fn list_tile(
        &mut self,
        scope: Scope,
        title: impl Into<ReactiveValue<String>>,
        subtitle: Option<impl Into<ReactiveValue<String>>>,
        leading: Option<impl Into<ReactiveValue<String>>>,
        trailing: Option<impl Into<ReactiveValue<String>>>,
        on_tap: Option<impl FnMut() + 'static>,
    ) -> NodeId {
        let title_rv = title.into();
        let subtitle_rv = subtitle.map(|s| s.into());
        let leading_rv = leading.map(|l| l.into());
        let trailing_rv = trailing.map(|t| t.into());
        let h = if subtitle_rv.is_some() { 72.0 } else { 56.0 };
        let layout = LayoutStyle::row()
            .with_size(Length::Pct(100.0), Length::Dp(h))
            .with_align(Alignment::Center);
        let id = self.spawn_node(
            "ListTile",
            WidgetKind::ListTile {
                leading: leading_rv,
                title: title_rv.clone(),
                subtitle: subtitle_rv,
                trailing: trailing_rv,
                on_tap: on_tap.map(|cb| Rc::new(RefCell::new(cb)) as TapHandler),
                hovered: false,
                pressed: false,
                selected: ReactiveValue::Static(false),
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::ListItem;
            n.a11y.label = Some(title_rv.get());
            n.a11y.focusable = true;
        }
        id
    }

    /// Mount a Material 3 selectable or action chip matching Flutter's `Chip` / `FilterChip`.
    pub fn chip(
        &mut self,
        scope: Scope,
        label: impl Into<ReactiveValue<String>>,
        selected: Signal<bool>,
        icon: Option<impl Into<ReactiveValue<String>>>,
        on_tap: Option<impl FnMut() + 'static>,
    ) -> NodeId {
        let label_rv = label.into();
        let icon_rv = icon.map(|i| i.into());
        let layout = LayoutStyle::row()
            .with_size(Length::Auto, Length::Dp(32.0))
            .with_padding(Insets::symmetric(12.0, 4.0))
            .with_align(Alignment::Center);
        let id = self.spawn_node(
            "Chip",
            WidgetKind::Chip {
                label: label_rv.clone(),
                icon: icon_rv,
                selected,
                on_tap: on_tap.map(|cb| Rc::new(RefCell::new(cb)) as TapHandler),
                hovered: false,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::CheckBox;
            n.a11y.label = Some(label_rv.get());
            n.a11y.focusable = true;
        }
        id
    }

    /// Mount a Material 3 Top App Bar header matching Flutter's `AppBar`.
    pub fn app_bar(
        &mut self,
        scope: Scope,
        title: impl Into<ReactiveValue<String>>,
        leading_icon: Option<impl Into<ReactiveValue<String>>>,
        elevation: f32,
        on_leading_tap: Option<impl FnMut() + 'static>,
    ) -> NodeId {
        let title_rv = title.into();
        let leading_rv = leading_icon.map(|l| l.into());
        let layout = LayoutStyle::row()
            .with_size(Length::Pct(100.0), Length::Dp(64.0))
            .with_padding(Insets::symmetric(16.0, 8.0))
            .with_align(Alignment::Center);
        let id = self.spawn_node(
            "AppBar",
            WidgetKind::AppBar {
                title: title_rv.clone(),
                leading_icon: leading_rv,
                elevation,
                on_leading_tap: on_leading_tap.map(|cb| Rc::new(RefCell::new(cb)) as TapHandler),
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Container;
            n.a11y.label = Some(title_rv.get());
        }
        id
    }

    /// Mount a slide-out navigation drawer panel matching Flutter's `Drawer`.
    pub fn drawer(
        &mut self,
        scope: Scope,
        open: Signal<bool>,
        width: f32,
        on_close: Option<impl FnMut() + 'static>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        let id = self.spawn_node(
            "Drawer",
            WidgetKind::Drawer {
                open,
                width,
                on_close: on_close.map(|cb| Rc::new(RefCell::new(cb)) as TapHandler),
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Dialog;
            n.a11y.label = Some("Navigation Drawer".to_string());
        }
        id
    }

    /// Compose a Flutter-style Scaffold layout managing app bar, body, bottom nav, FAB, and drawer.
    pub fn scaffold(
        &mut self,
        scope: Scope,
        app_bar: Option<NodeId>,
        body: NodeId,
        bottom_nav_bar: Option<NodeId>,
        floating_action_button: Option<NodeId>,
        drawer: Option<NodeId>,
    ) -> NodeId {
        let root_layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        let root = self.container(scope, root_layout, BoxStyle::default());
        if let Some(ab) = app_bar {
            self.append_child(root, ab);
        }
        if let Some(body_node) = self.nodes.get_mut(body) {
            body_node.layout.flex_grow = 1.0;
        }
        self.append_child(root, body);
        if let Some(bnb) = bottom_nav_bar {
            self.append_child(root, bnb);
        }
        if let Some(fab) = floating_action_button {
            if let Some(fab_node) = self.nodes.get_mut(fab) {
                fab_node.layout.absolute = true;
                fab_node.layout.inset_bottom = Some(24.0);
                fab_node.layout.inset_right = Some(24.0);
            }
            self.append_child(root, fab);
        }
        if let Some(dr) = drawer {
            if let Some(dr_node) = self.nodes.get_mut(dr) {
                dr_node.layout.absolute = true;
                dr_node.layout.use_inset = true;
                dr_node.layout.inset = Insets::ZERO;
            }
            self.append_child(root, dr);
        }
        root
    }

    /// Mount any custom user-defined widget implementing [`CustomWidget`].
    pub fn custom_widget<W: CustomWidget + 'static>(
        &mut self,
        scope: Scope,
        widget: W,
        layout: LayoutStyle,
    ) -> NodeId {
        let dbg = widget.debug_name();
        let a11y = widget.a11y();
        let id = self.spawn_node(
            dbg,
            WidgetKind::Custom(Rc::new(RefCell::new(widget))),
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y = a11y;
        }
        id
    }

    fn effective_taffy_layout(n: &WidgetNode) -> LayoutStyle {
        let mut st = n.layout.clone();
        if n.taffy_collapsed_override {
            st.height = Length::Dp(44.0);
            st.min_height = Length::Dp(44.0);
            st.max_height = Length::Dp(44.0);
            st.padding.top = 0.0;
            st.padding.bottom = 0.0;
        }
        if n.taffy_hidden_by_parent {
            st.hidden = true;
        }
        st
    }

    pub fn set_layout(&mut self, node: NodeId, mut layout: LayoutStyle) {
        if let Some(n) = self.nodes.get_mut(node) {
            if matches!(n.kind, WidgetKind::SafeArea) {
                layout.safe_area = true;
            }
            if matches!(n.kind, WidgetKind::Accordion { .. }) {
                layout.header_top_inset = 44.0;
            }
            n.layout = layout;
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            let eff = Self::effective_taffy_layout(n);
            self.layout_engine.upsert_node(node, eff);
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn set_box_style(&mut self, node: NodeId, box_style: BoxStyle) {
        if !self.nodes.contains_key(node) {
            return;
        }
        if let Some(bg) = box_style.background.clone()
            && bg.is_dynamic()
        {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.box_bg) {
                eff_scope.effect(move || {
                    let _ = bg.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.box_bg);
        }
        if let Some(bc) = box_style.border_color.clone()
            && bc.is_dynamic()
        {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.box_border_color) {
                eff_scope.effect(move || {
                    let _ = bc.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.box_border_color);
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style = box_style;
            n.dirty |= DirtyFlags::PAINT;
        }
        self.sync_node_anim_targets(node);
    }

    pub fn set_transform(&mut self, node: NodeId, transform: Transform2D) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.transform = transform;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_backdrop_blur(&mut self, node: NodeId, blur_radius: f32, tint: Color) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.backdrop_blur = blur_radius.max(0.0);
            n.box_style.backdrop_tint = tint;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_shadows(&mut self, node: NodeId, shadows: Vec<BoxShadow>) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.shadows = shadows;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_blend_mode(&mut self, node: NodeId, blend_mode: BlendMode) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.blend_mode = blend_mode;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_cursor(&mut self, node: NodeId, cursor: Option<CursorIcon>) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.cursor = cursor;
        }
    }

    /// Replace draw and/or pointer callbacks on an existing [`WidgetKind::Canvas`] node.
    pub fn set_canvas_callbacks(
        &mut self,
        node: NodeId,
        on_paint: Option<crate::widgets::CanvasPaintFn>,
        on_pointer: Option<CanvasPointerFn>,
    ) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Canvas {
                on_paint: p_slot,
                on_pointer: ptr_slot,
            } = &mut n.kind
        {
            if let Some(p) = on_paint {
                *p_slot = p;
            }
            if let Some(ptr) = on_pointer {
                *ptr_slot = Some(ptr);
            }
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    /// Bind a reactive property updater that re-runs during `layout()` whenever any tracked signal changes.
    pub fn bind_dynamic_prop(
        &mut self,
        node: NodeId,
        dirty_mask: DirtyFlags,
        track: impl Fn() + 'static,
        mut apply: impl FnMut(NodeId, &mut WidgetTree) + 'static,
    ) {
        if !self.nodes.contains_key(node) {
            return;
        }
        apply(node, self);

        let Some(node_scope) = self.nodes.get(node).map(|n| n.scope) else {
            return;
        };
        let eff_scope = node_scope.child_scope();
        eff_scope.effect(move || {
            track();
            mark_node_dirty(node, dirty_mask);
        });
        if !self.prop_scopes.contains_key(node) {
            self.prop_scopes.insert(node, PropEffectScopes::default());
        }
        if let Some(scopes) = self.prop_scopes.get_mut(node) {
            scopes.dynamic_props.push(eff_scope);
        }
        if !self.dynamic_prop_updaters.contains_key(node) {
            self.dynamic_prop_updaters.insert(node, Vec::new());
        }
        if let Some(updaters) = self.dynamic_prop_updaters.get_mut(node) {
            updaters.push((dirty_mask, Box::new(apply)));
        }
    }

    /// Configure a smooth transition curve for a specific property on `node`.
    pub fn set_transition(&mut self, node: NodeId, prop: AnimatableProp, curve: TransitionCurve) {
        let Some(n) = self.nodes.get(node) else {
            return;
        };
        let initial_opacity = n.box_style.opacity;
        let initial_bg = n
            .box_style
            .background
            .as_ref()
            .and_then(|rv| match rv.get() {
                Brush::Solid(c) => Some(c),
                _ => None,
            });
        let initial_bc = n
            .box_style
            .border_color
            .as_ref()
            .map(ReactiveValue::get)
            .or_else(|| n.box_style.border.map(|b| b.color));

        if !self.node_anims.contains_key(node) {
            self.node_anims.insert(node, NodeAnimState::default());
        }
        let Some(anim) = self.node_anims.get_mut(node) else {
            return;
        };
        anim.curves.insert(prop, curve);
        match prop {
            AnimatableProp::Opacity => {
                if let Some(a) = anim.opacity.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.opacity = Some(AnimatedFloat::new(initial_opacity, curve));
                }
            }
            AnimatableProp::TranslateX => {
                if let Some(a) = anim.translate_x.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.translate_x = Some(AnimatedFloat::new(0.0, curve));
                }
            }
            AnimatableProp::TranslateY => {
                if let Some(a) = anim.translate_y.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.translate_y = Some(AnimatedFloat::new(0.0, curve));
                }
            }
            AnimatableProp::ScaleX => {
                if let Some(a) = anim.scale_x.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.scale_x = Some(AnimatedFloat::new(1.0, curve));
                }
            }
            AnimatableProp::ScaleY => {
                if let Some(a) = anim.scale_y.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.scale_y = Some(AnimatedFloat::new(1.0, curve));
                }
            }
            AnimatableProp::Rotation => {
                if let Some(a) = anim.rotation.as_mut() {
                    a.set_curve(curve);
                } else {
                    anim.rotation = Some(AnimatedFloat::new(0.0, curve));
                }
            }
            AnimatableProp::BackgroundColor => {
                if let Some(ac) = anim.bg_color.as_mut() {
                    ac.set_curve(curve);
                } else if let Some(c) = initial_bg {
                    anim.bg_color = Some(AnimatedColor::new(c, curve));
                }
            }
            AnimatableProp::BorderColor => {
                if let Some(ac) = anim.border_color.as_mut() {
                    ac.set_curve(curve);
                } else if let Some(c) = initial_bc {
                    anim.border_color = Some(AnimatedColor::new(c, curve));
                }
            }
        }
    }

    pub fn set_opacity(&mut self, node: NodeId, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if let Some(n) = self.nodes.get_mut(node) {
            n.box_style.opacity = clamped;
            n.dirty |= DirtyFlags::PAINT;
        }
        if let Some(anim) = self.node_anims.get_mut(node)
            && let Some(&curve) = anim.curves.get(&AnimatableProp::Opacity)
        {
            if let Some(a) = anim.opacity.as_mut() {
                a.set_curve(curve);
                a.set_target(clamped);
            } else {
                let mut a = AnimatedFloat::new(clamped, curve);
                a.set_target(clamped);
                anim.opacity = Some(a);
            }
        }
    }

    pub fn set_translate(&mut self, node: NodeId, dx: f32, dy: f32) {
        if !self.nodes.contains_key(node) {
            return;
        }
        if !self.node_anims.contains_key(node) {
            if dx == 0.0 && dy == 0.0 {
                return;
            }
            self.node_anims.insert(node, NodeAnimState::default());
        }
        if let Some(anim) = self.node_anims.get_mut(node) {
            if let Some(&cx) = anim.curves.get(&AnimatableProp::TranslateX) {
                let slot = anim
                    .translate_x
                    .get_or_insert_with(|| AnimatedFloat::new(0.0, cx));
                slot.set_curve(cx);
                slot.set_target(dx);
            } else {
                let slot = anim
                    .translate_x
                    .get_or_insert_with(|| AnimatedFloat::new(dx, TransitionCurve::instant()));
                slot.snap(dx);
            }
            if let Some(&cy) = anim.curves.get(&AnimatableProp::TranslateY) {
                let slot = anim
                    .translate_y
                    .get_or_insert_with(|| AnimatedFloat::new(0.0, cy));
                slot.set_curve(cy);
                slot.set_target(dy);
            } else {
                let slot = anim
                    .translate_y
                    .get_or_insert_with(|| AnimatedFloat::new(dy, TransitionCurve::instant()));
                slot.snap(dy);
            }
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_scale(&mut self, node: NodeId, sx: f32, sy: f32) {
        if !self.nodes.contains_key(node) {
            return;
        }
        if !self.node_anims.contains_key(node) {
            if (sx - 1.0).abs() < 1e-6 && (sy - 1.0).abs() < 1e-6 {
                return;
            }
            self.node_anims.insert(node, NodeAnimState::default());
        }
        if let Some(anim) = self.node_anims.get_mut(node) {
            if let Some(&cx) = anim.curves.get(&AnimatableProp::ScaleX) {
                let slot = anim
                    .scale_x
                    .get_or_insert_with(|| AnimatedFloat::new(1.0, cx));
                slot.set_curve(cx);
                slot.set_target(sx);
            } else {
                let slot = anim
                    .scale_x
                    .get_or_insert_with(|| AnimatedFloat::new(sx, TransitionCurve::instant()));
                slot.snap(sx);
            }
            if let Some(&cy) = anim.curves.get(&AnimatableProp::ScaleY) {
                let slot = anim
                    .scale_y
                    .get_or_insert_with(|| AnimatedFloat::new(1.0, cy));
                slot.set_curve(cy);
                slot.set_target(sy);
            } else {
                let slot = anim
                    .scale_y
                    .get_or_insert_with(|| AnimatedFloat::new(sy, TransitionCurve::instant()));
                slot.snap(sy);
            }
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_rotation(&mut self, node: NodeId, radians: f32) {
        if !self.nodes.contains_key(node) {
            return;
        }
        if !self.node_anims.contains_key(node) {
            if radians == 0.0 {
                return;
            }
            self.node_anims.insert(node, NodeAnimState::default());
        }
        if let Some(anim) = self.node_anims.get_mut(node) {
            if let Some(&cr) = anim.curves.get(&AnimatableProp::Rotation) {
                let slot = anim
                    .rotation
                    .get_or_insert_with(|| AnimatedFloat::new(0.0, cr));
                slot.set_curve(cr);
                slot.set_target(radians);
            } else {
                let slot = anim
                    .rotation
                    .get_or_insert_with(|| AnimatedFloat::new(radians, TransitionCurve::instant()));
                slot.snap(radians);
            }
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    fn sync_node_anim_targets(&mut self, node: NodeId) {
        let Some(n) = self.nodes.get(node) else {
            return;
        };
        let Some(anim) = self.node_anims.get_mut(node) else {
            return;
        };
        if let Some(&curve) = anim.curves.get(&AnimatableProp::Opacity) {
            let slot = anim
                .opacity
                .get_or_insert_with(|| AnimatedFloat::new(n.box_style.opacity, curve));
            slot.set_target(n.box_style.opacity.clamp(0.0, 1.0));
        }
        if let Some(&curve) = anim.curves.get(&AnimatableProp::BackgroundColor)
            && let Some(ref bg) = n.box_style.background
            && let Brush::Solid(target_c) = bg.get()
        {
            if let Some(ac) = anim.bg_color.as_mut() {
                ac.set_target(target_c);
            } else {
                anim.bg_color = Some(AnimatedColor::new(target_c, curve));
            }
        }
        if let Some(&curve) = anim.curves.get(&AnimatableProp::BorderColor) {
            let target_bc = n
                .box_style
                .border_color
                .as_ref()
                .map(ReactiveValue::get)
                .or_else(|| n.box_style.border.map(|b| b.color));
            if let Some(target_c) = target_bc {
                if let Some(ac) = anim.border_color.as_mut() {
                    ac.set_target(target_c);
                } else {
                    anim.border_color = Some(AnimatedColor::new(target_c, curve));
                }
            }
        }
    }

    fn node_effective_transform(&self, id: NodeId, pivot: Point) -> Transform2D {
        let base = self
            .nodes
            .get(id)
            .map(|n| n.box_style.transform)
            .unwrap_or(Transform2D::IDENTITY);
        let anim = self.node_anims.get(id);
        let sx = anim
            .and_then(|a| a.scale_x.as_ref())
            .map_or(1.0, |v| v.value);
        let sy = anim
            .and_then(|a| a.scale_y.as_ref())
            .map_or(1.0, |v| v.value);
        let rot = anim
            .and_then(|a| a.rotation.as_ref())
            .map_or(0.0, |v| v.value);
        let tx = anim
            .and_then(|a| a.translate_x.as_ref())
            .map_or(0.0, |v| v.value);
        let ty = anim
            .and_then(|a| a.translate_y.as_ref())
            .map_or(0.0, |v| v.value);

        let has_scale = (sx - 1.0).abs() > 1e-5 || (sy - 1.0).abs() > 1e-5;
        let has_rot = rot.abs() > 1e-5;
        let has_trans = tx.abs() > 1e-5 || ty.abs() > 1e-5;
        if base.is_identity() && !has_scale && !has_rot && !has_trans {
            return Transform2D::IDENTITY;
        }
        let mut inner = base;
        if has_scale {
            inner = inner.then(Transform2D::scale(sx, sy));
        }
        if has_rot {
            inner = inner.then(Transform2D::rotation(rot));
        }
        let mut xf = if inner.is_identity() {
            Transform2D::IDENTITY
        } else {
            Transform2D::around_pivot(pivot, inner)
        };
        if has_trans {
            xf = xf.then(Transform2D::translation(tx, ty));
        }
        xf
    }

    fn default_cursor_for_node(node: &WidgetNode) -> CursorIcon {
        if node.is_disabled() {
            return CursorIcon::NotAllowed;
        }
        match &node.kind {
            WidgetKind::Button { .. }
            | WidgetKind::FloatingActionButton { .. }
            | WidgetKind::Checkbox { .. }
            | WidgetKind::Switch { .. }
            | WidgetKind::Tabs { .. }
            | WidgetKind::SegmentedControl { .. }
            | WidgetKind::BottomNavBar { .. }
            | WidgetKind::Accordion { .. }
            | WidgetKind::DataTable { .. }
            | WidgetKind::Select { .. }
            | WidgetKind::RadioGroup { .. } => CursorIcon::Pointer,
            WidgetKind::TextInput { .. } => CursorIcon::Text,
            WidgetKind::Slider { .. } => CursorIcon::Grab,
            WidgetKind::SplitPane { axis, .. } => match axis {
                Axis::Horizontal => CursorIcon::ResizeHorizontal,
                Axis::Vertical => CursorIcon::ResizeVertical,
            },
            _ if node.on_tap.is_some()
                || node.on_double_tap.is_some()
                || node.on_long_press.is_some() =>
            {
                CursorIcon::Pointer
            }
            _ => CursorIcon::Default,
        }
    }

    /// Resolve cursor shape hint for a hovered node, checking active drags and ancestor overrides first.
    pub fn hovered_cursor(&self) -> CursorIcon {
        self.cursor_for_node_at(self.hovered, self.last_pointer_pos)
    }

    /// Hit-test `point` and return the effective [`CursorIcon`] under the pointer.
    pub fn cursor_at(&self, point: Point) -> CursorIcon {
        let hit = self.hit_test(point);
        self.cursor_for_node_at(hit, Some(point))
    }

    fn cursor_for_node_at(&self, start: Option<NodeId>, point_opt: Option<Point>) -> CursorIcon {
        for n in self.nodes.values() {
            if let WidgetKind::SplitPane {
                axis,
                dragging: true,
                ..
            } = &n.kind
            {
                return match axis {
                    Axis::Horizontal => CursorIcon::ResizeHorizontal,
                    Axis::Vertical => CursorIcon::ResizeVertical,
                };
            }
        }
        if self
            .control_anims
            .values()
            .any(|anim| matches!(anim, ControlVisualAnim::Slider { dragging: true, .. }))
        {
            return CursorIcon::Grabbing;
        }

        if let Some(id) = start {
            if let Some(ControlVisualAnim::Slider { dragging: true, .. }) =
                self.control_anims.get(id)
            {
                return CursorIcon::Grabbing;
            }
            let mut cur = Some(id);
            while let Some(nid) = cur {
                if let Some(n) = self.nodes.get(nid) {
                    if let Some(c) = n.box_style.cursor {
                        return if n.is_disabled() {
                            CursorIcon::NotAllowed
                        } else {
                            c
                        };
                    }
                    if nid == id {
                        if let WidgetKind::SplitPane {
                            axis,
                            ratio,
                            min_first,
                            min_second,
                            divider_size,
                            ..
                        } = &n.kind
                        {
                            if let Some(pt) = point_opt {
                                let local_pt = self.screen_to_node_local_point(nid, pt);
                                let inner = self.node_visual_rect(nid).inset(n.layout.padding);
                                let (_, _, div_rect) = Self::split_divider_geometry(
                                    inner,
                                    *axis,
                                    ratio.get_untracked(),
                                    *min_first,
                                    *min_second,
                                    *divider_size,
                                );
                                if div_rect.inflate(4.0, 4.0).contains(local_pt) {
                                    return match axis {
                                        Axis::Horizontal => CursorIcon::ResizeHorizontal,
                                        Axis::Vertical => CursorIcon::ResizeVertical,
                                    };
                                }
                            } else {
                                return match axis {
                                    Axis::Horizontal => CursorIcon::ResizeHorizontal,
                                    Axis::Vertical => CursorIcon::ResizeVertical,
                                };
                            }
                        } else if let WidgetKind::Accordion { .. } = &n.kind {
                            if let Some(pt) = point_opt {
                                let local_pt = self.screen_to_node_local_point(nid, pt);
                                let base_rect = self.node_visual_rect(nid);
                                if local_pt.y <= base_rect.y + 44.0 {
                                    return CursorIcon::Pointer;
                                }
                                return CursorIcon::Default;
                            }
                            return CursorIcon::Pointer;
                        } else if let WidgetKind::ToastHost { items, .. } = &n.kind {
                            if let Some(pt) = point_opt {
                                let local_pt = self.screen_to_node_local_point(nid, pt);
                                let base_rect = self.node_visual_rect(nid);
                                let (_, closes) =
                                    Self::toast_card_rects(base_rect, items.get().len());
                                if closes
                                    .iter()
                                    .any(|cr| cr.inflate(6.0, 6.0).contains(local_pt))
                                {
                                    return CursorIcon::Pointer;
                                }
                            }
                        } else {
                            let sem = Self::default_cursor_for_node(n);
                            if sem != CursorIcon::Default {
                                return sem;
                            }
                        }
                    }
                    cur = n.parent;
                } else {
                    break;
                }
            }
        }
        CursorIcon::Default
    }

    pub fn set_text_input_style(&mut self, node: NodeId, style: TextStyle) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::TextInput { style: st, .. } = &mut n.kind
        {
            *st = style;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_button_foreground(&mut self, node: NodeId, fg: impl Into<ReactiveValue<Color>>) {
        let rv = fg.into();
        let is_btn = self
            .nodes
            .get(node)
            .is_some_and(|n| matches!(n.kind, WidgetKind::Button { .. }));
        if !is_btn {
            return;
        }
        if rv.is_dynamic() {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.button_fg) {
                let rv_c = rv.clone();
                eff_scope.effect(move || {
                    let _ = rv_c.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.button_fg);
        }
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Button { foreground, .. } = &mut n.kind
        {
            *foreground = Some(rv);
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_disabled(&mut self, node: NodeId, disabled: impl Into<ReactiveValue<bool>>) {
        let rv = disabled.into();
        if !self.nodes.contains_key(node) {
            return;
        }
        if rv.is_dynamic() {
            if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.disabled) {
                let rv_c = rv.clone();
                eff_scope.effect(move || {
                    let _ = rv_c.get();
                    mark_node_dirty(node, DirtyFlags::PAINT | DirtyFlags::A11Y);
                });
            }
        } else {
            self.dispose_prop_effect(node, |s| &mut s.disabled);
        }
        if let Some(n) = self.nodes.get_mut(node) {
            if let WidgetKind::Button { disabled: d, .. } = &mut n.kind {
                *d = rv.clone();
            }
            n.disabled = Some(rv);
            n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        }
    }

    pub fn set_image_radius(&mut self, node: NodeId, rad: CornerRadii) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Image { radius, .. } = &mut n.kind
        {
            *radius = rad;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_on_tap(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: TapHandler = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Button { on_tap, .. }
                | WidgetKind::FloatingActionButton { on_tap, .. } => {
                    *on_tap = Some(rc);
                }
                _ => {
                    n.on_tap = Some(rc);
                }
            }
        }
    }

    pub fn set_on_double_tap(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_double_tap = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_long_press(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_long_press = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_hover(&mut self, node: NodeId, cb: impl FnMut(bool) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_hover = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_pinch_rotate(&mut self, node: NodeId, cb: impl FnMut(Point, f32, f32) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_pinch_rotate = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_change(&mut self, node: NodeId, cb: impl FnMut(String) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::TextInput { on_change, .. } = &mut n.kind
        {
            *on_change = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_bool_change(&mut self, node: NodeId, cb: impl FnMut(bool) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: BoolCallback = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Checkbox { on_change, .. }
                | WidgetKind::Switch { on_change, .. }
                | WidgetKind::Accordion { on_change, .. } => {
                    *on_change = Some(rc);
                }
                _ => {}
            }
        }
    }

    pub fn set_on_float_change(&mut self, node: NodeId, cb: impl FnMut(f32) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: FloatCallback = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Slider { on_change, .. } | WidgetKind::SplitPane { on_change, .. } => {
                    *on_change = Some(rc);
                }
                _ => {}
            }
        }
    }

    pub fn set_on_index_change(&mut self, node: NodeId, cb: impl FnMut(usize) + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: IndexCallback = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Tabs { on_change, .. }
                | WidgetKind::SegmentedControl { on_change, .. }
                | WidgetKind::BottomNavBar { on_change, .. }
                | WidgetKind::Select { on_change, .. }
                | WidgetKind::RadioGroup { on_change, .. } => {
                    *on_change = Some(rc);
                }
                WidgetKind::DataTable { on_select, .. }
                | WidgetKind::PopupMenu { on_select, .. } => {
                    *on_select = Some(rc);
                }
                _ => {}
            }
        }
    }

    pub fn set_on_submit(&mut self, node: NodeId, cb: impl FnMut(String) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::TextInput { on_submit, .. } = &mut n.kind
        {
            *on_submit = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_close(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            let rc: TapHandler = Rc::new(RefCell::new(cb));
            match &mut n.kind {
                WidgetKind::Overlay { on_close, .. } | WidgetKind::PopupMenu { on_close, .. } => {
                    *on_close = Some(rc);
                }
                _ => {}
            }
        }
    }

    /// Reactively toggle whether `node` participates in layout and painting.
    pub fn set_visible_when(&mut self, node: NodeId, pred: impl Fn() -> bool + 'static) {
        let rv = ReactiveValue::Dynamic(Rc::new(pred));
        if !self.nodes.contains_key(node) {
            return;
        }
        if let Some(eff_scope) = self.reset_prop_effect(node, |s| &mut s.visible_when) {
            let rv_c = rv.clone();
            eff_scope.effect(move || {
                let _ = rv_c.get();
                mark_node_dirty(node, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
            });
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.visible_when = Some(rv);
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
        }
    }

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

    fn sync_dynamic_slots(&mut self) -> bool {
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

    fn sync_dynamic_props(&mut self, drained_masks: &HashMap<NodeId, DirtyFlags>) {
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

    pub fn rect(&self, node: NodeId) -> Rect {
        self.layout_engine.rect(node)
    }

    /// Whether a node is mounted and shown: neither it nor any ancestor is
    /// hidden by `visible`, a false `if` condition, a closed overlay or a
    /// collapsed accordion.
    pub fn is_visible(&self, node: NodeId) -> bool {
        !self.is_effectively_hidden(node)
    }

    fn is_effectively_hidden(&self, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(nid) = cur {
            let Some(node) = self.nodes.get(nid) else {
                return true;
            };
            if node.layout.hidden || node.taffy_hidden_by_parent {
                return true;
            }
            if node
                .visible_when
                .as_ref()
                .is_some_and(|v| craftui_reactive::untrack(|| !v.get()))
            {
                return true;
            }
            match &node.kind {
                WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                    if craftui_reactive::untrack(|| !open.get()) =>
                {
                    return true;
                }
                WidgetKind::Accordion { expanded, .. }
                    if nid != id && craftui_reactive::untrack(|| !expanded.get()) =>
                {
                    return true;
                }
                _ => {}
            }
            cur = node.parent;
        }
        false
    }

    // === Frame Step: Animations, Dirty Sync, Layout, Virtualization, Paint, A11y ===

    /// Advance spring animations, inertial scroll flings, and return whether another animation frame is needed.
    pub fn tick_animations(&mut self, dt_sec: f32) -> bool {
        let mut active = false;

        if let Some((from, to, mut spring)) = self.theme_spring.take() {
            let still_running = spring.step(dt_sec);
            let lerped = from.lerp(&to, spring.value);
            self.theme_signal.set(lerped);
            if still_running {
                self.theme_spring = Some((from, to, spring));
                active = true;
            }
        }

        let anim_ids: Vec<NodeId> = self.node_anims.keys().collect();
        for id in anim_ids {
            if let Some(anim) = self.node_anims.get_mut(id) {
                let (changed, still_running) = anim.step(dt_sec);
                if changed && let Some(n) = self.nodes.get_mut(id) {
                    n.dirty |= DirtyFlags::PAINT;
                }
                if still_running {
                    active = true;
                }
            }
        }

        let ctrl_ids: Vec<NodeId> = self.control_anims.keys().collect();
        for id in ctrl_ids {
            if let Some(anim) = self.control_anims.get_mut(id) {
                let changed = anim.step(dt_sec);
                if changed && let Some(n) = self.nodes.get_mut(id) {
                    n.dirty |= DirtyFlags::PAINT;
                }
                if anim.is_animating() {
                    active = true;
                }
            }
        }

        let ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in ids {
            let rect = self.layout_engine.rect(id);
            let effectively_hidden = self.is_effectively_hidden(id);
            let physics = self.scroll_physics;
            if let Some(node) = self.nodes.get_mut(id) {
                match &mut node.kind {
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        if velocity.abs() > 0.0 {
                            let (delta, running) = physics.step_fling(velocity, dt_sec);
                            let viewport = match axis {
                                Axis::Vertical => rect.height,
                                Axis::Horizontal => rect.width,
                            };
                            let max_off = (*content_extent - viewport).max(0.0);
                            *offset = (*offset - delta).clamp(0.0, max_off);
                            if *offset <= 0.0 || *offset >= max_off {
                                *velocity = 0.0;
                            }
                            node.dirty |= DirtyFlags::PAINT;
                            if running && *velocity != 0.0 {
                                active = true;
                            }
                        }
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        if velocity.abs() > 0.0 {
                            let (delta, running) = physics.step_fling(velocity, dt_sec);
                            let total_rows = item_count.get().div_ceil((*columns).max(1));
                            let total_extent = total_rows as f32 * *item_extent;
                            let viewport = match axis {
                                Axis::Vertical => rect.height,
                                Axis::Horizontal => rect.width,
                            };
                            let max_off = (total_extent - viewport).max(0.0);
                            *offset = (*offset - delta).clamp(0.0, max_off);
                            node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                            if running {
                                active = true;
                            }
                        }
                    }
                    WidgetKind::ProgressIndicator { progress, phase }
                        if !effectively_hidden && progress.get().is_none() =>
                    {
                        *phase = (*phase + dt_sec * 1.4) % 1.0;
                        node.dirty |= DirtyFlags::PAINT;
                        active = true;
                    }
                    WidgetKind::Spinner { phase, .. } if !effectively_hidden => {
                        *phase = (*phase + dt_sec * 1.25) % 1.0;
                        node.dirty |= DirtyFlags::PAINT;
                        active = true;
                    }
                    _ => {}
                }
            }
        }

        active
    }

    /// Returns `true` if any transition, control spring, theme transition, fling, or spinner is currently running.
    pub fn has_active_animations(&self) -> bool {
        if self.theme_spring.is_some() {
            return true;
        }
        if self.node_anims.values().any(NodeAnimState::is_animating) {
            return true;
        }
        if self
            .control_anims
            .values()
            .any(ControlVisualAnim::is_animating)
        {
            return true;
        }
        self.nodes.iter().any(|(id, node)| match &node.kind {
            WidgetKind::ScrollView { velocity, .. } | WidgetKind::VirtualList { velocity, .. } => {
                velocity.abs() > 0.0
            }
            WidgetKind::ProgressIndicator { progress, .. } => {
                !self.is_effectively_hidden(id) && progress.get().is_none()
            }
            WidgetKind::Spinner { .. } => !self.is_effectively_hidden(id),
            _ => false,
        })
    }

    /// Update tree window metrics and notify reactive listeners.
    pub fn set_metrics(&mut self, metrics: WindowMetrics) {
        self.metrics = metrics;
        self.metrics_signal.set(metrics);
    }

    /// Pull dirty flags from `craftui-reactive` and update layout + virtualized windows.
    pub fn layout(&mut self, metrics: WindowMetrics) -> usize {
        self.metrics = metrics;
        // Platform runners assign `metrics` directly before the first frame, so
        // comparing against the field alone would leave the reactive copy at
        // its default and `Window.*` bindings would see the wrong size.
        if self.metrics_signal.get_untracked() != metrics {
            self.metrics_signal.set(metrics);
        }
        let dirty_updates = drain_dirty_nodes();
        let dirty_count = dirty_updates.len();

        let mut drained_masks: HashMap<NodeId, DirtyFlags> = HashMap::new();
        for (id, flags) in dirty_updates {
            if let Some(node) = self.nodes.get_mut(id) {
                node.dirty |= flags;
                drained_masks
                    .entry(id)
                    .and_modify(|f| *f |= flags)
                    .or_insert(flags);
                if flags.contains(DirtyFlags::LAYOUT) {
                    self.layout_engine.mark_dirty(id);
                }
            }
        }

        // Rebuild any reactive dynamic_slot / reactive_for subtrees before syncing properties/solving layout
        self.sync_dynamic_slots();
        for (id, flags) in drain_dirty_nodes() {
            if let Some(node) = self.nodes.get_mut(id) {
                node.dirty |= flags;
                drained_masks
                    .entry(id)
                    .and_modify(|f| *f |= flags)
                    .or_insert(flags);
                if flags.contains(DirtyFlags::LAYOUT) {
                    self.layout_engine.mark_dirty(id);
                }
            }
        }
        self.sync_dynamic_props(&drained_masks);

        // Sync dynamic properties before layout
        let all_ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in all_ids {
            if let Some(node) = self.nodes.get_mut(id) {
                let mut layout_changed = false;
                if let Some(ref vis) = node.visible_when {
                    let hidden = !vis.get();
                    if node.layout.hidden != hidden {
                        node.layout.hidden = hidden;
                        layout_changed = true;
                    }
                }
                let node_disabled = node.disabled.as_ref().is_some_and(|d| d.get());
                node.a11y.disabled = node_disabled;
                match &mut node.kind {
                    WidgetKind::Text { content, style } => {
                        if node.dirty.contains(DirtyFlags::TEXT)
                            || node.dirty.contains(DirtyFlags::LAYOUT)
                        {
                            node.cached_text = content.get();
                            node.cached_style = style.get();
                            node.a11y.label = Some(node.cached_text.clone());
                        }
                    }
                    WidgetKind::Button {
                        label, disabled, ..
                    } => {
                        let txt = label.get();
                        node.cached_text = txt.clone();
                        node.a11y.label = Some(txt);
                        node.a11y.disabled = node_disabled || disabled.get();
                    }
                    WidgetKind::TextInput {
                        editor,
                        bound_signal: Some(sig),
                        ..
                    } => {
                        let next = sig.get_untracked();
                        if editor.text != next {
                            editor.sync_external(&next);
                            node.a11y.value = Some(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        }
                    }
                    WidgetKind::Checkbox { checked, .. } | WidgetKind::Switch { checked, .. } => {
                        node.a11y.checked = Some(checked.get_untracked());
                    }
                    WidgetKind::Slider { value, .. } => {
                        node.a11y.numeric_value = Some(value.get_untracked() as f64);
                    }
                    WidgetKind::Tabs { active, .. } => {
                        node.a11y.numeric_value = Some(active.get_untracked() as f64);
                    }
                    WidgetKind::SegmentedControl { selected, .. }
                    | WidgetKind::BottomNavBar { selected, .. } => {
                        node.a11y.numeric_value = Some(selected.get_untracked() as f64);
                    }
                    WidgetKind::FloatingActionButton {
                        label,
                        icon,
                        disabled,
                        ..
                    } => {
                        let l = label
                            .as_ref()
                            .map(|s| s.get())
                            .unwrap_or_else(|| icon.get());
                        node.cached_text = l.clone();
                        node.a11y.label = Some(l);
                        node.a11y.disabled = node_disabled || disabled.get();
                    }
                    WidgetKind::Accordion {
                        title, expanded, ..
                    } => {
                        node.a11y.label = Some(title.get());
                        node.a11y.checked = Some(expanded.get_untracked());
                    }
                    WidgetKind::DataTable { selected, .. } => {
                        node.a11y.numeric_value = Some(selected.get_untracked() as f64);
                    }
                    WidgetKind::Select {
                        options, selected, ..
                    } => {
                        let opts = options.get();
                        let idx = selected.get_untracked();
                        node.a11y.value = opts.get(idx).cloned();
                    }
                    WidgetKind::RadioGroup { selected, .. } => {
                        node.a11y.numeric_value = Some(selected.get_untracked() as f64);
                    }
                    WidgetKind::Tooltip { text } => {
                        node.a11y.label = Some(text.get());
                    }
                    WidgetKind::Badge { label, .. } => {
                        node.a11y.label = Some(label.get());
                    }
                    WidgetKind::ToastHost { items, .. } => {
                        let list = items.get();
                        node.a11y.value =
                            list.last().map(|t| format!("{}: {}", t.title, t.message));
                    }
                    WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. } => {
                        let should_show = open.get();
                        let hidden = !should_show;
                        if node.layout.hidden != hidden {
                            node.layout.hidden = hidden;
                            layout_changed = true;
                        }
                    }
                    WidgetKind::Custom(cw) => {
                        if let Ok(guard) = cw.try_borrow() {
                            node.a11y = guard.a11y();
                        }
                    }
                    _ => {}
                }
                if layout_changed {
                    let st = Self::effective_taffy_layout(node);
                    self.layout_engine.upsert_node(id, st);
                    self.layout_engine.mark_dirty(id);
                }
            }
            self.sync_control_anim_target(id);
            self.sync_node_anim_targets(id);
        }

        let Some(root) = self.root else {
            return dirty_count;
        };

        self.sync_accordions();

        // Pass 1: solve layout
        self.solve_taffy(root, metrics);

        // Pass 2: recycle VirtualList / VirtualGrid visible items & sync SplitPane ratios
        let virtual_changed = self.sync_virtual_lists();
        let split_changed = self.sync_split_panes();
        if virtual_changed || split_changed {
            self.solve_taffy(root, metrics);
        }

        // Update ScrollView content_extent
        let scroll_ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in scroll_ids {
            let is_scroll = self
                .nodes
                .get(id)
                .is_some_and(|n| matches!(n.kind, WidgetKind::ScrollView { .. }));
            if is_scroll {
                let rect = self.layout_engine.rect(id);
                let mut stack = self
                    .nodes
                    .get(id)
                    .map(|n| n.children.clone())
                    .unwrap_or_default();
                let mut max_bottom = 0.0f32;
                let mut max_right = 0.0f32;
                while let Some(c) = stack.pop() {
                    let cr = self.layout_engine.rect(c);
                    max_bottom = max_bottom.max(cr.max_y() - rect.y);
                    max_right = max_right.max(cr.max_x() - rect.x);
                    if let Some(cn) = self.nodes.get(c) {
                        stack.extend(cn.children.iter().copied());
                    }
                }
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::ScrollView {
                        axis,
                        offset,
                        content_extent,
                        ..
                    } = &mut n.kind
                {
                    *content_extent = match axis {
                        Axis::Vertical => max_bottom,
                        Axis::Horizontal => max_right,
                    };
                    let view = match axis {
                        Axis::Vertical => rect.height,
                        Axis::Horizontal => rect.width,
                    };
                    let max_off = (*content_extent - view).max(0.0);
                    *offset = offset.clamp(0.0, max_off);
                }
            }
        }

        dirty_count
    }

    fn solve_taffy(&mut self, root: NodeId, metrics: WindowMetrics) {
        let nodes = &self.nodes;
        let text_engine = &mut self.text_engine;
        let theme = self.theme_signal.get_untracked();
        let safe_area = metrics.effective_safe_area();

        self.layout_engine.compute_layout(
            root,
            metrics.logical_size.width,
            metrics.logical_size.height,
            safe_area,
            metrics.scale_factor,
            self.direction,
            |node_id, constraints| {
                let Some(node) = nodes.get(node_id) else {
                    return Size::ZERO;
                };
                match &node.kind {
                    WidgetKind::Divider {
                        axis, thickness, ..
                    } => match axis {
                        Axis::Horizontal => Size::new(100.0, *thickness),
                        Axis::Vertical => Size::new(*thickness, 24.0),
                    },
                    WidgetKind::Text { content, style } => {
                        let txt = if node.cached_text.is_empty() {
                            content.get()
                        } else {
                            node.cached_text.clone()
                        };
                        let mut st = style.get();
                        if st.color == Color::TRANSPARENT {
                            st.color = theme.text_primary;
                        }
                        let max_w = if constraints.max_width.is_finite() {
                            Some(constraints.max_width)
                        } else {
                            None
                        };
                        text_engine.measure(&txt, &st, max_w)
                    }
                    WidgetKind::Icon { size, .. } => Size::new(*size, *size),
                    WidgetKind::Button { label, .. } => {
                        let txt = label.get();
                        if builtin_icon_svg(&txt).is_some() {
                            Size::new(20.0, 20.0)
                        } else {
                            let st = TextStyle::default().with_size(15.0).with_weight(600.0);
                            text_engine.measure(&txt, &st, None)
                        }
                    }
                    WidgetKind::Checkbox { label, .. } => {
                        let s =
                            text_engine.measure(label, &TextStyle::default().with_size(14.0), None);
                        Size::new(26.0 + s.width, s.height.max(22.0))
                    }
                    WidgetKind::Switch { label, .. } => {
                        let s =
                            text_engine.measure(label, &TextStyle::default().with_size(14.0), None);
                        Size::new(50.0 + s.width, s.height.max(26.0))
                    }
                    WidgetKind::Slider { label, .. } => {
                        Size::new(160.0, if label.is_empty() { 32.0 } else { 48.0 })
                    }
                    WidgetKind::TextInput { multiline, .. } => {
                        Size::new(180.0, if *multiline { 104.0 } else { 40.0 })
                    }
                    WidgetKind::Tabs { tabs, .. } => {
                        let list = tabs.get();
                        let total_w: f32 = list
                            .iter()
                            .map(|t| {
                                text_engine
                                    .measure(t, &TextStyle::default().with_size(14.0), None)
                                    .width
                                    + 28.0
                            })
                            .sum();
                        Size::new(total_w.max(160.0), 40.0)
                    }
                    WidgetKind::SegmentedControl { items, .. } => {
                        let list = items.get();
                        let total_w: f32 = list
                            .iter()
                            .map(|t| {
                                text_engine
                                    .measure(t, &TextStyle::default().with_size(13.5), None)
                                    .width
                                    + 24.0
                            })
                            .sum();
                        Size::new(total_w.max(140.0), 36.0)
                    }
                    WidgetKind::BottomNavBar { .. } => {
                        let w = if constraints.max_width.is_finite() {
                            constraints.max_width
                        } else {
                            360.0
                        };
                        Size::new(w, 64.0)
                    }
                    WidgetKind::FloatingActionButton { label, icon, .. } => {
                        if let Some(lbl) = label {
                            let l = lbl.get();
                            let i = icon.get();
                            let st = TextStyle::default().with_size(14.0).with_weight(600.0);
                            let isz = text_engine.measure(&i, &st, None);
                            let lsz = text_engine.measure(&l, &st, None);
                            Size::new((isz.width + lsz.width + 36.0).max(96.0), 52.0)
                        } else {
                            Size::new(56.0, 56.0)
                        }
                    }
                    WidgetKind::HtmlView { .. } => {
                        let w = if constraints.max_width.is_finite() {
                            constraints.max_width
                        } else {
                            400.0
                        };
                        let h = if constraints.max_height.is_finite() {
                            constraints.max_height
                        } else {
                            300.0
                        };
                        Size::new(w, h)
                    }
                    WidgetKind::Accordion {
                        title, expanded, ..
                    } => {
                        let title_w = text_engine
                            .measure(
                                &title.get(),
                                &TextStyle::default().with_size(14.0).with_weight(600.0),
                                None,
                            )
                            .width;
                        let h = if expanded.get_untracked() { 0.0 } else { 44.0 };
                        Size::new((title_w + 52.0).max(180.0), h)
                    }
                    WidgetKind::DataTable {
                        columns,
                        rows,
                        row_height,
                        ..
                    } => {
                        let col_count = columns.get().len().max(1);
                        let row_count = rows.get().len();
                        let w = (col_count as f32 * 120.0).max(240.0);
                        let h = 38.0 + (row_count as f32 * *row_height).min(320.0).max(*row_height);
                        Size::new(w, h)
                    }
                    WidgetKind::Select { .. } => Size::new(180.0, 40.0),
                    WidgetKind::RadioGroup { options, axis, .. } => {
                        let opts = options.get();
                        let count = opts.len().max(1);
                        let st = TextStyle::default().with_size(14.0);
                        match axis {
                            Axis::Vertical => {
                                let max_w = opts
                                    .iter()
                                    .map(|o| 28.0 + text_engine.measure(o, &st, None).width)
                                    .fold(100.0f32, f32::max);
                                Size::new(max_w, count as f32 * 30.0)
                            }
                            Axis::Horizontal => {
                                let total_w: f32 = opts
                                    .iter()
                                    .map(|o| 36.0 + text_engine.measure(o, &st, None).width)
                                    .sum();
                                Size::new(total_w.max(120.0), 32.0)
                            }
                        }
                    }
                    WidgetKind::ProgressIndicator { .. } => Size::new(120.0, 8.0),
                    WidgetKind::Spinner { size, .. } => Size::new(*size, *size),
                    WidgetKind::Badge { label, icon, .. } => {
                        let txt = label.get();
                        let st = TextStyle::default().with_size(12.0).with_weight(600.0);
                        let measured = text_engine.measure(&txt, &st, None);
                        let icon_w = if icon.as_ref().is_some_and(|i| !i.get().is_empty()) {
                            16.0
                        } else {
                            0.0
                        };
                        Size::new(measured.width + icon_w, measured.height.max(16.0))
                    }
                    WidgetKind::ListTile { subtitle, .. } => {
                        let w = if constraints.max_width.is_finite() {
                            constraints.max_width
                        } else {
                            320.0
                        };
                        let h = if subtitle.is_some() { 72.0 } else { 56.0 };
                        Size::new(w, h)
                    }
                    WidgetKind::Chip { label, icon, .. } => {
                        let txt = label.get();
                        let st = TextStyle::default().with_size(13.0).with_weight(500.0);
                        let measured = text_engine.measure(&txt, &st, None);
                        let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
                        Size::new((measured.width + icon_w + 24.0).max(48.0), 32.0)
                    }
                    WidgetKind::AppBar { .. } => {
                        let w = if constraints.max_width.is_finite() {
                            constraints.max_width
                        } else {
                            360.0
                        };
                        Size::new(w, 64.0)
                    }
                    WidgetKind::Drawer { width, .. } => {
                        let h = if constraints.max_height.is_finite() {
                            constraints.max_height
                        } else {
                            800.0
                        };
                        Size::new(*width, h)
                    }
                    WidgetKind::Custom(cw) => {
                        if let Ok(mut guard) = cw.try_borrow_mut() {
                            guard.measure(constraints, text_engine, &theme)
                        } else {
                            Size::new(100.0, 40.0)
                        }
                    }
                    _ => Size::ZERO,
                }
            },
        );
    }

    fn sync_virtual_lists(&mut self) -> bool {
        let mut changed = false;
        let v_ids: Vec<NodeId> = self.virtual_hosts.keys().collect();

        for host_id in v_ids {
            let rect = self.layout_engine.rect(host_id);
            let Some(node) = self.nodes.get(host_id) else {
                continue;
            };
            let WidgetKind::VirtualList {
                axis,
                columns,
                item_extent,
                item_count,
                offset,
                ..
            } = &node.kind
            else {
                continue;
            };

            let axis = *axis;
            let total_count = item_count.get();
            let cols = (*columns).max(1);
            let extent = *item_extent;
            let off = *offset;
            let viewport = match axis {
                Axis::Vertical => rect.height.max(extent),
                Axis::Horizontal => rect.width.max(extent),
            };
            let overscan_rows = 2usize;
            let first_row =
                ((off / extent).floor() as isize - overscan_rows as isize).max(0) as usize;
            let last_row = (((off + viewport) / extent).ceil() as usize + overscan_rows)
                .min(total_count.div_ceil(cols).saturating_sub(1));

            let first_idx = (first_row * cols).min(total_count);
            let last_idx = ((last_row + 1) * cols).min(total_count);
            let new_range = if total_count == 0 {
                None
            } else {
                Some((first_idx, last_idx))
            };

            let needs_rebuild = self
                .virtual_hosts
                .get(host_id)
                .is_some_and(|h| h.last_range != new_range);
            if !needs_rebuild {
                continue;
            }
            changed = true;

            let Some(mut host) = self.virtual_hosts.remove(host_id) else {
                continue;
            };
            host.last_range = new_range;

            // Dispose out-of-window items
            let mut to_remove = Vec::new();
            for (&idx, &(item_node, item_scope)) in &host.active_items {
                let keep = new_range.is_some_and(|(s, e)| idx >= s && idx < e);
                if !keep {
                    to_remove.push((idx, item_node, item_scope));
                }
            }
            for (idx, item_node, _item_scope) in to_remove {
                host.active_items.remove(&idx);
                self.remove_subtree(item_node);
            }

            let host_scope = self
                .nodes
                .get(host_id)
                .map(|n| n.scope)
                .unwrap_or(self.root_scope);
            let mut ordered_children = Vec::new();

            if let Some((start, end)) = new_range {
                let col_w = if cols > 1 {
                    (rect.width / cols as f32).max(20.0)
                } else {
                    rect.width.max(20.0)
                };
                for idx in start..end {
                    let row = idx / cols;
                    let col = idx % cols;
                    let item_node = if let Some(&(existing, _)) = host.active_items.get(&idx) {
                        existing
                    } else {
                        let item_scope = host_scope.child_scope();
                        let created = (host.factory)(item_scope, idx, self);
                        host.active_items.insert(idx, (created, item_scope));
                        created
                    };

                    // Position each recycled item absolutely at its virtualized slot
                    let mut st = self
                        .nodes
                        .get(item_node)
                        .map(|n| n.layout.clone())
                        .unwrap_or_default();
                    st.absolute = true;
                    st.use_inset = true;
                    match axis {
                        Axis::Vertical => {
                            st.inset = Insets::new(
                                row as f32 * extent - off,
                                0.0,
                                0.0,
                                col as f32 * col_w,
                            );
                            st.width = Length::Dp(col_w);
                            st.height = Length::Dp(extent);
                        }
                        Axis::Horizontal => {
                            st.inset = Insets::new(0.0, 0.0, 0.0, idx as f32 * extent - off);
                            st.width = Length::Dp(extent);
                            st.height = Length::Dp(rect.height.max(20.0));
                        }
                    }
                    self.set_layout(item_node, st);
                    ordered_children.push(item_node);
                }
            }

            self.set_children(host_id, ordered_children);
            self.virtual_hosts.insert(host_id, host);
        }

        changed
    }

    fn split_divider_geometry(
        inner_rect: Rect,
        axis: Axis,
        ratio: f32,
        min_first: f32,
        min_second: f32,
        divider_size: f32,
    ) -> (f32, f32, Rect) {
        let div = divider_size.max(4.0);
        let total = match axis {
            Axis::Horizontal => inner_rect.width,
            Axis::Vertical => inner_rect.height,
        };
        let usable = (total - div).max(0.0);
        let min_f = min_first.max(0.0).min(usable);
        let max_f = (usable - min_second.max(0.0)).max(min_f);
        let first_px = (usable * ratio.clamp(0.0, 1.0)).clamp(min_f, max_f);
        let second_px = (usable - first_px).max(0.0);
        let div_rect = match axis {
            Axis::Horizontal => Rect::new(
                inner_rect.x + first_px,
                inner_rect.y,
                div,
                inner_rect.height,
            ),
            Axis::Vertical => {
                Rect::new(inner_rect.x, inner_rect.y + first_px, inner_rect.width, div)
            }
        };
        (first_px, second_px, div_rect)
    }

    fn sync_split_panes(&mut self) -> bool {
        let mut changed = false;
        let split_ids: Vec<NodeId> = self
            .nodes
            .iter()
            .filter_map(|(id, n)| matches!(n.kind, WidgetKind::SplitPane { .. }).then_some(id))
            .collect();

        for id in split_ids {
            let rect = self.layout_engine.rect(id);
            let Some((axis, ratio_val, min_first, min_second, divider_size, c0, c1, pad)) =
                self.nodes.get(id).and_then(|n| {
                    if n.layout.hidden {
                        return None;
                    }
                    if let WidgetKind::SplitPane {
                        axis,
                        ratio,
                        min_first,
                        min_second,
                        divider_size,
                        ..
                    } = &n.kind
                        && n.children.len() >= 2
                    {
                        Some((
                            *axis,
                            ratio.get_untracked(),
                            *min_first,
                            *min_second,
                            *divider_size,
                            n.children[0],
                            n.children[1],
                            n.layout.padding,
                        ))
                    } else {
                        None
                    }
                })
            else {
                continue;
            };

            let inner = rect.inset(pad);
            if inner.width <= 0.0 && inner.height <= 0.0 {
                continue;
            }
            let (first_px, second_px, _) = Self::split_divider_geometry(
                inner,
                axis,
                ratio_val,
                min_first,
                min_second,
                divider_size,
            );

            for (cid, target_dp) in [(c0, first_px), (c1, second_px)] {
                if let Some(cn) = self.nodes.get_mut(cid) {
                    let mut st = cn.layout.clone();
                    let mut child_changed = false;
                    let want = Length::Dp(target_dp);
                    match axis {
                        Axis::Horizontal => {
                            let diff = match st.width {
                                Length::Dp(a) => (a - target_dp).abs() > 0.25,
                                _ => true,
                            };
                            if diff {
                                st.width = want;
                                child_changed = true;
                            }
                        }
                        Axis::Vertical => {
                            let diff = match st.height {
                                Length::Dp(a) => (a - target_dp).abs() > 0.25,
                                _ => true,
                            };
                            if diff {
                                st.height = want;
                                child_changed = true;
                            }
                        }
                    }
                    if st.flex_grow != 0.0 {
                        st.flex_grow = 0.0;
                        child_changed = true;
                    }
                    if st.flex_shrink != 0.0 {
                        st.flex_shrink = 0.0;
                        child_changed = true;
                    }
                    if child_changed {
                        cn.layout = st.clone();
                        cn.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                        self.layout_engine.upsert_node(cid, st);
                        self.layout_engine.mark_dirty(cid);
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    fn sync_accordions(&mut self) {
        let mut collapsed_set = HashSet::new();
        let mut collapsed_children = HashSet::new();
        let all_ids: Vec<NodeId> = self.nodes.keys().collect();
        for &id in &all_ids {
            if let Some(n) = self.nodes.get(id)
                && let WidgetKind::Accordion { expanded, .. } = &n.kind
                && !expanded.get_untracked()
            {
                collapsed_set.insert(id);
                for &c in &n.children {
                    collapsed_children.insert(c);
                }
            }
        }
        for id in all_ids {
            let want_collapsed = collapsed_set.contains(&id);
            let want_hidden = collapsed_children.contains(&id);
            let mut changed = false;
            let mut eff_opt = None;
            if let Some(n) = self.nodes.get_mut(id) {
                if n.taffy_collapsed_override != want_collapsed {
                    n.taffy_collapsed_override = want_collapsed;
                    changed = true;
                }
                if n.taffy_hidden_by_parent != want_hidden {
                    n.taffy_hidden_by_parent = want_hidden;
                    changed = true;
                }
                if changed {
                    n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                    eff_opt = Some(Self::effective_taffy_layout(n));
                }
            }
            if let Some(eff) = eff_opt {
                self.layout_engine.upsert_node(id, eff);
                self.layout_engine.mark_dirty(id);
            }
        }
    }

    fn data_table_sorted_indices(
        rows: &[Vec<String>],
        sort_column: Option<usize>,
        sort_ascending: bool,
    ) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..rows.len()).collect();
        if let Some(col) = sort_column {
            indices.sort_by(|&a, &b| {
                let sa = rows
                    .get(a)
                    .and_then(|r| r.get(col))
                    .map(String::as_str)
                    .unwrap_or("");
                let sb = rows
                    .get(b)
                    .and_then(|r| r.get(col))
                    .map(String::as_str)
                    .unwrap_or("");
                let ord = match (sa.trim().parse::<f64>(), sb.trim().parse::<f64>()) {
                    (Ok(na), Ok(nb)) => na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal),
                    _ => sa.to_lowercase().cmp(&sb.to_lowercase()),
                };
                if sort_ascending { ord } else { ord.reverse() }
            });
        }
        indices
    }

    fn toast_card_rects(container_rect: Rect, count: usize) -> (Vec<Rect>, Vec<Rect>) {
        let mut cards = Vec::with_capacity(count);
        let mut closes = Vec::with_capacity(count);
        if count == 0 {
            return (cards, closes);
        }
        let card_w = (container_rect.width - 32.0).clamp(180.0, 340.0);
        let card_h = 64.0f32;
        let gap = 10.0f32;
        for i in 0..count {
            let from_bottom = count - 1 - i;
            let y = container_rect.max_y() - 16.0 - card_h - (from_bottom as f32) * (card_h + gap);
            let x = container_rect.max_x() - 16.0 - card_w;
            let card_r = Rect::new(x, y, card_w, card_h);
            let close_r = Rect::new(card_r.max_x() - 28.0, card_r.y + 10.0, 18.0, 18.0);
            cards.push(card_r);
            closes.push(close_r);
        }
        (cards, closes)
    }

    fn select_dropdown_rect(&self, trigger_rect: Rect, option_count: usize) -> Rect {
        let count = option_count.max(1);
        let h = (count as f32 * 34.0 + 8.0).min(240.0);
        let vh = self.metrics.logical_size.height.max(200.0);
        let y = if trigger_rect.max_y() + 4.0 + h <= vh {
            trigger_rect.max_y() + 4.0
        } else {
            (trigger_rect.y - 4.0 - h).max(4.0)
        };
        Rect::new(trigger_rect.x, y, trigger_rect.width.max(140.0), h)
    }

    fn popup_menu_rect(
        &self,
        anchor: Option<Point>,
        item_count: usize,
        container_rect: Rect,
    ) -> Rect {
        let count = item_count.max(1);
        let w = 210.0f32;
        let h = (count as f32 * 36.0 + 8.0).min(320.0);
        let base =
            anchor.unwrap_or_else(|| Point::new(container_rect.x + 24.0, container_rect.y + 48.0));
        let max_x = (self.metrics.logical_size.width - w - 8.0).max(8.0);
        let max_y = (self.metrics.logical_size.height - h - 8.0).max(8.0);
        Rect::new(base.x.clamp(8.0, max_x), base.y.clamp(8.0, max_y), w, h)
    }

    /// Record [`PaintScene`] commands from the retained widget tree.
    pub fn paint(&mut self, scene: &mut PaintScene) {
        scene.clear();
        let Some(root) = self.root else {
            return;
        };
        let theme = self.theme_signal.get_untracked();
        let mut floating_selects: Vec<(Rect, Vec<String>, usize, f32)> = Vec::new();
        let mut active_tooltips: Vec<(Rect, String)> = Vec::new();
        self.paint_node_recursive(
            root,
            Point::ZERO,
            Transform2D::IDENTITY,
            &theme,
            scene,
            &mut floating_selects,
            &mut active_tooltips,
        );

        // Paint floating Select dropdown popups on top of main UI
        for (drop_rect, opts, sel_idx, dropdown_scroll) in floating_selects {
            scene.drop_shadow(
                drop_rect,
                CornerRadii::all(8.0),
                BoxShadow::drop(0.0, 4.0, 12.0, 0.0, Color::rgba(0, 0, 0, 110)),
            );
            scene.fill_rect(drop_rect, theme.surface, CornerRadii::all(8.0));
            scene.stroke_rect(drop_rect, theme.border, 1.0, CornerRadii::all(8.0));
            let total_h = opts.len() as f32 * 34.0 + 8.0;
            let max_scroll = (total_h - drop_rect.height).max(0.0);
            let scroll = dropdown_scroll.clamp(0.0, max_scroll);
            scene.push(PaintCommand::PushClip {
                rect: drop_rect.inset(Insets::all(2.0)),
                radius: CornerRadii::all(6.0),
            });
            for (i, opt) in opts.iter().enumerate() {
                let row_y = drop_rect.y + 4.0 + i as f32 * 34.0 - scroll;
                if row_y + 32.0 < drop_rect.y || row_y > drop_rect.max_y() {
                    continue;
                }
                let item_r = Rect::new(drop_rect.x + 4.0, row_y, drop_rect.width - 8.0, 32.0);
                if i == sel_idx {
                    scene.fill_rect(
                        item_r,
                        theme.primary.with_alpha(0.18),
                        CornerRadii::all(6.0),
                    );
                }
                scene.draw_text(
                    Point::new(item_r.x + 10.0, item_r.y + 8.0),
                    opt,
                    TextStyle::default()
                        .with_size(14.0)
                        .with_weight(if i == sel_idx { 600.0 } else { 400.0 })
                        .with_color(if i == sel_idx {
                            theme.primary
                        } else {
                            theme.text_primary
                        }),
                    Some((item_r.width - 20.0).max(20.0)),
                );
            }
            if max_scroll > 0.5 {
                let ratio = (drop_rect.height / total_h).clamp(0.12, 1.0);
                let track_h = (drop_rect.height - 8.0).max(8.0);
                let thumb_h = (track_h * ratio).max(16.0);
                let pos_ratio = (scroll / max_scroll).clamp(0.0, 1.0);
                let thumb_y = drop_rect.y + 4.0 + (track_h - thumb_h) * pos_ratio;
                scene.fill_rect(
                    Rect::new(drop_rect.max_x() - 6.0, thumb_y, 3.5, thumb_h),
                    theme.text_secondary.with_alpha(0.55),
                    CornerRadii::all(2.0),
                );
            }
            scene.push(PaintCommand::PopClip);
        }

        // Paint active hover tooltips on top of main UI & dropdowns
        for (target_rect, tip_text) in active_tooltips {
            if tip_text.is_empty() {
                continue;
            }
            let st = TextStyle::default()
                .with_size(12.0)
                .with_color(Color::WHITE);
            let measured = self.text_engine.measure(&tip_text, &st, Some(240.0));
            let w = (measured.width + 18.0).max(40.0);
            let h = measured.height + 10.0;
            let tx = (target_rect.center().x - w * 0.5)
                .clamp(6.0, (self.metrics.logical_size.width - w - 6.0).max(6.0));
            let ty = if target_rect.y >= h + 10.0 {
                target_rect.y - h - 6.0
            } else {
                target_rect.max_y() + 6.0
            };
            let tip_rect = Rect::new(tx, ty, w, h);
            scene.drop_shadow(
                tip_rect,
                CornerRadii::all(6.0),
                BoxShadow::drop(0.0, 3.0, 8.0, 0.0, Color::rgba(0, 0, 0, 90)),
            );
            scene.fill_rect(
                tip_rect,
                Color::rgba(15, 23, 42, 240),
                CornerRadii::all(6.0),
            );
            scene.stroke_rect(tip_rect, theme.border, 1.0, CornerRadii::all(6.0));
            scene.draw_text(Point::new(tx + 9.0, ty + 5.0), &tip_text, st, Some(240.0));
        }

        if self.inspector.enabled {
            let mut boxes = Vec::new();
            for (id, node) in &self.nodes {
                let r = self.layout_engine.rect(id);
                if !r.size().is_empty() {
                    let xr = self
                        .node_world_transform(id)
                        .transform_rect_bounds(self.node_visual_rect(id));
                    boxes.push((xr, node.debug_name));
                }
            }
            self.inspector
                .paint_overlay(scene, &boxes, self.metrics.logical_size.width);
        }
    }

    #[inline]
    fn union_rects(a: Rect, b: Rect) -> Rect {
        let min_x = a.x.min(b.x);
        let min_y = a.y.min(b.y);
        let max_x = a.max_x().max(b.max_x());
        let max_y = a.max_y().max(b.max_y());
        Rect::new(
            min_x,
            min_y,
            (max_x - min_x).max(0.0),
            (max_y - min_y).max(0.0),
        )
    }

    #[inline]
    fn expand_rect_for_shadow(rect: Rect, s: BoxShadow) -> Rect {
        if s.inset {
            return rect;
        }
        let pad = s.blur_radius.max(0.0) * 2.5 + s.spread.max(0.0) + 4.0;
        let sr = rect.translate(s.offset_x, s.offset_y).inflate(pad, pad);
        Self::union_rects(rect, sr)
    }

    fn accumulate_subtree_bounds(
        &self,
        id: NodeId,
        scroll_offset: Point,
        to_layer_local: Transform2D,
        bounds: &mut Rect,
    ) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return;
        }
        match &node.kind {
            WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                if !open.get() =>
            {
                return;
            }
            WidgetKind::Drawer { open, .. } if !open.get() => {
                return;
            }
            _ => {}
        }

        let rect = self
            .layout_engine
            .rect(id)
            .translate(scroll_offset.x, scroll_offset.y);
        let local_xf = self.node_effective_transform(id, rect.center());
        let xform = to_layer_local * local_xf;

        let mut local_r = rect.inflate(4.0, 4.0);
        if let Some(s) = node.box_style.shadow {
            local_r = Self::expand_rect_for_shadow(local_r, s);
        }
        for &s in &node.box_style.shadows {
            local_r = Self::expand_rect_for_shadow(local_r, s);
        }
        let mapped = if xform.is_identity() {
            local_r
        } else {
            xform.transform_rect_bounds(local_r)
        };
        *bounds = Self::union_rects(*bounds, mapped);

        if node.box_style.clip
            || matches!(
                &node.kind,
                WidgetKind::Accordion { expanded, .. } if !expanded.get_untracked()
            )
        {
            return;
        }
        let mut child_scroll = scroll_offset;
        if let WidgetKind::ScrollView { axis, offset, .. } = &node.kind {
            match axis {
                Axis::Vertical => child_scroll.y -= *offset,
                Axis::Horizontal => child_scroll.x -= *offset,
            }
        }
        for &child in &node.children {
            self.accumulate_subtree_bounds(child, child_scroll, xform, bounds);
        }
    }

    fn compute_subtree_layer_bounds(
        &self,
        _id: NodeId,
        rect: Rect,
        child_offset: Point,
        box_style: &BoxStyle,
        children: &[NodeId],
    ) -> Rect {
        let mut bounds = rect.inflate(4.0, 4.0);
        if let Some(s) = box_style.shadow {
            bounds = Self::expand_rect_for_shadow(bounds, s);
        }
        for &s in &box_style.shadows {
            bounds = Self::expand_rect_for_shadow(bounds, s);
        }
        if !box_style.clip {
            for &child in children {
                self.accumulate_subtree_bounds(
                    child,
                    child_offset,
                    Transform2D::IDENTITY,
                    &mut bounds,
                );
            }
        }
        bounds
    }

    fn hovered_has_ancestor(&self, ancestor: NodeId) -> bool {
        let mut cursor = self.hovered;
        while let Some(id) = cursor {
            if id == ancestor {
                return true;
            }
            cursor = self.nodes.get(id).and_then(|n| n.parent);
        }
        false
    }

    fn adapt_brush_to_rect(brush: &Brush, rect: Rect) -> Brush {
        match brush {
            Brush::Solid(c) => Brush::Solid(*c),
            Brush::LinearGradient { start, end, stops } => {
                let s = if start.x <= 1.0 && start.y <= 1.0 && rect.width > 1.0 && rect.height > 1.0
                {
                    Point::new(
                        rect.x + start.x * rect.width,
                        rect.y + start.y * rect.height,
                    )
                } else {
                    *start
                };
                let e = if end.x <= 1.0 && end.y <= 1.0 && rect.width > 1.0 && rect.height > 1.0 {
                    Point::new(rect.x + end.x * rect.width, rect.y + end.y * rect.height)
                } else {
                    *end
                };
                Brush::LinearGradient {
                    start: s,
                    end: e,
                    stops: stops.clone(),
                }
            }
            Brush::RadialGradient {
                center,
                radius,
                stops,
            } => {
                let c = if center.x <= 1.0
                    && center.y <= 1.0
                    && rect.width > 1.0
                    && rect.height > 1.0
                {
                    Point::new(
                        rect.x + center.x * rect.width,
                        rect.y + center.y * rect.height,
                    )
                } else {
                    *center
                };
                let r = if *radius <= 1.0 && rect.width > 1.0 && rect.height > 1.0 {
                    *radius * rect.width.max(rect.height) * 0.5
                } else {
                    *radius
                };
                Brush::RadialGradient {
                    center: c,
                    radius: r,
                    stops: stops.clone(),
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_node_recursive(
        &mut self,
        id: NodeId,
        scroll_offset: Point,
        current_xform: Transform2D,
        theme: &ThemeTokens,
        scene: &mut PaintScene,
        floating_selects: &mut Vec<(Rect, Vec<String>, usize, f32)>,
        active_tooltips: &mut Vec<(Rect, String)>,
    ) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return;
        }
        let rect = self
            .layout_engine
            .rect(id)
            .translate(scroll_offset.x, scroll_offset.y);
        let box_style = node.box_style.clone();
        let node_padding = node.layout.padding;
        let children = node.children.clone();
        let kind = node.kind.clone();
        let is_focused = self.focused == Some(id) && self.focus_visible;
        let is_disabled = node.is_disabled();

        match &kind {
            WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                if !open.get() =>
            {
                return;
            }
            _ => {}
        }

        let node_anim = self.node_anims.get(id);
        let eff_opacity = node_anim
            .and_then(|a| a.opacity.as_ref())
            .map_or(box_style.opacity, |v| v.value)
            .clamp(0.0, 1.0);
        let anim_bg = node_anim.and_then(|a| a.bg_color.as_ref()).map(|c| c.value);
        let anim_border = node_anim
            .and_then(|a| a.border_color.as_ref())
            .map(|c| c.value);
        let ctrl_anim = self.control_anims.get(id).cloned();

        if eff_opacity <= 0.001 {
            if let Some(n) = self.nodes.get_mut(id) {
                n.dirty = DirtyFlags::NONE;
            }
            return;
        }

        let base_xf = self.node_effective_transform(id, rect.center());
        let button_press_xf = if let Some(ControlVisualAnim::Button { press_t, .. }) = &ctrl_anim
            && press_t.value > 0.001
        {
            let s = 1.0 - 0.025 * press_t.value.clamp(0.0, 1.0);
            Transform2D::around_pivot(rect.center(), Transform2D::scale(s, s))
        } else {
            Transform2D::IDENTITY
        };
        let local_xf = base_xf * button_press_xf;
        let has_xform = !local_xf.is_identity();
        let node_xform = if has_xform {
            current_xform * local_xf
        } else {
            current_xform
        };
        if has_xform {
            scene.push_transform(local_xf);
        }

        // Backdrop blur must execute BEFORE PushLayer because pooled offscreen layers start clear.
        if box_style.backdrop_blur > 0.2 {
            let mut tint = box_style.backdrop_tint;
            tint.a = ((tint.a as f32) * eff_opacity).round().clamp(0.0, 255.0) as u8;
            scene.backdrop_blur(rect, box_style.radius, box_style.backdrop_blur, tint);
        }

        let mut child_offset = scroll_offset;
        if let WidgetKind::ScrollView { axis, offset, .. } = &kind {
            match axis {
                Axis::Vertical => child_offset.y -= *offset,
                Axis::Horizontal => child_offset.x -= *offset,
            }
        }

        let needs_layer = eff_opacity < 0.999 || box_style.blend_mode != BlendMode::Normal;
        if needs_layer {
            let layer_bounds =
                self.compute_subtree_layer_bounds(id, rect, child_offset, &box_style, &children);
            scene.push_layer(layer_bounds, eff_opacity, box_style.blend_mode);
        }

        // Outer drop shadows
        for s in box_style
            .shadow
            .into_iter()
            .chain(box_style.shadows.iter().copied())
            .filter(|s| !s.inset)
        {
            scene.drop_shadow(rect, box_style.radius, s);
        }

        // Background fill (Button & FAB paint their own interactive hover/press fill once)
        if !matches!(
            kind,
            WidgetKind::Button { .. } | WidgetKind::FloatingActionButton { .. }
        ) {
            let effective_bg = match (anim_bg, &box_style.background) {
                (Some(c), Some(rv)) if matches!(rv.get(), Brush::Solid(_)) => Some(Brush::Solid(c)),
                (_, Some(rv)) => Some(rv.get()),
                (Some(c), None) => Some(Brush::Solid(c)),
                (None, None) => None,
            };
            if let Some(bg) = effective_bg {
                let adapted = Self::adapt_brush_to_rect(&bg, rect);
                scene.fill_rect(rect, adapted, box_style.radius);
            }
        }

        // Inset inner shadows on top of background fill
        for s in box_style
            .shadow
            .into_iter()
            .chain(box_style.shadows.iter().copied())
            .filter(|s| s.inset)
        {
            scene.drop_shadow(rect, box_style.radius, s);
        }

        let mut paint_children = true;
        let mut extra_overlay_anim: Option<(Transform2D, f32)> = None;

        match kind {
            WidgetKind::Container | WidgetKind::Spacer | WidgetKind::SafeArea => {}
            WidgetKind::Divider { axis, color, .. } => {
                let c = color.map(|cv| cv.get()).unwrap_or(theme.border);
                let draw_r = match axis {
                    Axis::Horizontal | Axis::Vertical => {
                        Rect::new(rect.x, rect.y, rect.width.max(1.0), rect.height.max(1.0))
                    }
                };
                scene.fill_rect(draw_r, c, CornerRadii::ZERO);
            }
            WidgetKind::Text { content, style } => {
                let txt = content.get();
                let mut st = style.get();
                if st.color == Color::TRANSPARENT {
                    st.color = theme.text_primary;
                }
                scene.draw_text(rect.origin(), txt, st, Some(rect.width.max(1.0)));
            }
            WidgetKind::Icon { svg, color, .. } => {
                let tint = color.map(|c| c.get()).unwrap_or(theme.text_primary);
                let _ = paint_svg_icon(scene, rect, &svg.get(), tint);
            }
            WidgetKind::Button {
                label,
                disabled,
                foreground,
                hovered,
                pressed,
                ..
            } => {
                let disabled_eff = is_disabled || disabled.get();
                let (hover_t, press_t) = match &ctrl_anim {
                    Some(ControlVisualAnim::Button { hover_t, press_t }) => {
                        (hover_t.value.clamp(0.0, 1.0), press_t.value.clamp(0.0, 1.0))
                    }
                    _ => (
                        if hovered { 1.0 } else { 0.0 },
                        if pressed { 1.0 } else { 0.0 },
                    ),
                };
                let bg_brush = if disabled_eff {
                    Brush::Solid(theme.surface_variant)
                } else {
                    match (anim_bg, &box_style.background) {
                        (Some(c), Some(rv)) if matches!(rv.get(), Brush::Solid(_)) => {
                            Brush::Solid(c)
                        }
                        (_, Some(rv)) => rv.get(),
                        (Some(c), None) => Brush::Solid(c),
                        (None, None) => Brush::Solid(theme.primary),
                    }
                };
                match bg_brush {
                    Brush::Solid(c) => {
                        let modulated = if disabled_eff {
                            c
                        } else {
                            c.lerp(Color::WHITE, 0.12 * hover_t)
                                .lerp(Color::BLACK, 0.22 * press_t)
                        };
                        scene.fill_rect(rect, modulated, box_style.radius);
                    }
                    other_brush => {
                        let adapted = Self::adapt_brush_to_rect(&other_brush, rect);
                        scene.fill_rect(rect, adapted, box_style.radius);
                        if !disabled_eff && hover_t > 0.01 {
                            scene.fill_rect(
                                rect,
                                Color::WHITE.with_alpha(0.12 * hover_t),
                                box_style.radius,
                            );
                        }
                        if !disabled_eff && press_t > 0.01 {
                            scene.fill_rect(
                                rect,
                                Color::BLACK.with_alpha(0.20 * press_t),
                                box_style.radius,
                            );
                        }
                    }
                }
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(1.5, 1.5),
                        theme.background,
                        1.5,
                        box_style.radius,
                    );
                    scene.stroke_rect(rect.inflate(3.5, 3.5), theme.primary, 2.0, box_style.radius);
                }
                if children.is_empty() {
                    let txt = label.get();
                    if !txt.is_empty() {
                        let fg = if disabled_eff {
                            theme.text_secondary
                        } else if let Some(ref custom_fg) = foreground {
                            custom_fg.get()
                        } else {
                            theme.on_primary
                        };
                        if builtin_icon_svg(&txt).is_some() {
                            let icon_sz =
                                (rect.width - 8.0).min(rect.height - 8.0).clamp(12.0, 24.0);
                            let ix = rect.x + (rect.width - icon_sz) * 0.5;
                            let iy = rect.y + (rect.height - icon_sz) * 0.5;
                            let icon_rect = Rect::new(ix, iy, icon_sz, icon_sz);
                            let _ = paint_svg_icon(scene, icon_rect, &txt, fg);
                        } else {
                            let tstyle = TextStyle::default()
                                .with_size(15.0)
                                .with_weight(600.0)
                                .with_color(fg);
                            let measured = self.text_engine.measure(&txt, &tstyle, None);
                            let tx = rect.x + (rect.width - measured.width) * 0.5;
                            let ty = rect.y + (rect.height - measured.height) * 0.5;
                            scene.draw_text(Point::new(tx, ty), txt, tstyle, None);
                        }
                    }
                }
            }
            WidgetKind::Image {
                texture,
                fit,
                radius,
                opacity,
            } => {
                if let Some(tex_id) = texture.get() {
                    scene.push(PaintCommand::DrawImage {
                        rect,
                        texture: tex_id,
                        fit,
                        radius,
                        opacity,
                    });
                } else {
                    scene.fill_rect(rect, theme.surface_variant, radius);
                }
            }
            WidgetKind::Canvas { on_paint, .. } => {
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                on_paint(rect, scene);
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::ScrollView {
                axis,
                offset,
                content_extent,
                ..
            } => {
                paint_children = false;
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                for child in children.iter().copied() {
                    self.paint_node_recursive(
                        child,
                        child_offset,
                        node_xform,
                        theme,
                        scene,
                        floating_selects,
                        active_tooltips,
                    );
                }
                let view = match axis {
                    Axis::Vertical => rect.height,
                    Axis::Horizontal => rect.width,
                };
                if content_extent > view + 1.0 {
                    let ratio = (view / content_extent).clamp(0.08, 1.0);
                    let pos_ratio = (offset / (content_extent - view)).clamp(0.0, 1.0);
                    match axis {
                        Axis::Vertical => {
                            let thumb_h = view * ratio;
                            let thumb_y = rect.y + (view - thumb_h) * pos_ratio;
                            scene.fill_rect(
                                Rect::new(rect.max_x() - 6.0, thumb_y, 4.0, thumb_h),
                                theme.text_secondary.with_alpha(0.55),
                                CornerRadii::all(2.0),
                            );
                        }
                        Axis::Horizontal => {
                            let thumb_w = view * ratio;
                            let thumb_x = rect.x + (view - thumb_w) * pos_ratio;
                            scene.fill_rect(
                                Rect::new(thumb_x, rect.max_y() - 6.0, thumb_w, 4.0),
                                theme.text_secondary.with_alpha(0.55),
                                CornerRadii::all(2.0),
                            );
                        }
                    }
                }
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::VirtualList { .. } => {
                paint_children = false;
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                for child in children.iter().copied() {
                    self.paint_node_recursive(
                        child,
                        scroll_offset,
                        node_xform,
                        theme,
                        scene,
                        floating_selects,
                        active_tooltips,
                    );
                }
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::TextInput {
                editor,
                placeholder,
                style,
                multiline,
                ..
            } => {
                scene.fill_rect(rect, theme.surface, box_style.radius);
                let is_input_active = self.focused == Some(id);
                let border_color = if is_input_active {
                    theme.primary
                } else {
                    theme.border
                };
                scene.stroke_rect(
                    rect,
                    border_color,
                    if is_input_active { 1.8 } else { 1.0 },
                    box_style.radius,
                );

                let pad = if multiline {
                    Insets::symmetric(12.0, 10.0)
                } else {
                    Insets::symmetric(12.0, 8.0)
                };
                let inner = rect.inset(pad);
                let wrap_w = if multiline {
                    Some(inner.width.max(40.0))
                } else {
                    None
                };
                scene.push(PaintCommand::PushClip {
                    rect: inner,
                    radius: CornerRadii::ZERO,
                });
                let disp = editor.display_text().into_owned();
                let mut draw_style = style.clone();
                let font_sz = draw_style.font_size;
                if disp.is_empty() {
                    draw_style.color = theme.text_secondary;
                    scene.draw_text(inner.origin(), placeholder, draw_style, wrap_w);
                    if is_input_active && self.caret_visible {
                        scene.fill_rect(
                            Rect::new(inner.x, inner.y, 2.0, font_sz * 1.25),
                            theme.primary,
                            CornerRadii::ZERO,
                        );
                    }
                } else {
                    draw_style.color = theme.text_primary;
                    let layout = self
                        .text_engine
                        .build_layout_cached(&disp, &draw_style, wrap_w);
                    if let Some((s, e)) = editor.selection_range() {
                        for sr in self.text_engine.selection_rects(&layout, s, e) {
                            scene.fill_rect(
                                sr.translate(inner.x, inner.y),
                                theme.primary.with_alpha(0.32),
                                CornerRadii::ZERO,
                            );
                        }
                    }
                    scene.draw_text(inner.origin(), &disp, draw_style, wrap_w);
                    if is_input_active && self.caret_visible {
                        let cr = self.text_engine.cursor_rect(&layout, editor.cursor);
                        scene.fill_rect(
                            Rect::new(inner.x + cr.x, inner.y + cr.y, 2.0, cr.height),
                            theme.primary,
                            CornerRadii::ZERO,
                        );
                    }
                }
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::Checkbox { checked, label, .. } => {
                let is_on = checked.get_untracked();
                let check_t = match &ctrl_anim {
                    Some(ControlVisualAnim::Checkbox { check_t }) => check_t.value.clamp(0.0, 1.0),
                    _ => {
                        if is_on {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                let box_rect = Rect::new(rect.x, rect.y + (rect.height - 20.0) * 0.5, 20.0, 20.0);
                let fill = theme.surface.lerp(theme.primary, check_t);
                let bcol = theme.border.lerp(theme.primary, check_t);
                scene.fill_rect(box_rect, fill, CornerRadii::all(5.0));
                scene.stroke_rect(box_rect, bcol, 1.5, CornerRadii::all(5.0));
                if check_t > 0.02 {
                    let p0 = (box_rect.x as f64 + 4.5, box_rect.y as f64 + 10.5);
                    let p1 = (box_rect.x as f64 + 8.5, box_rect.y as f64 + 14.5);
                    let p2 = (box_rect.x as f64 + 15.5, box_rect.y as f64 + 5.5);
                    let f1 = (check_t as f64 / 0.45).clamp(0.0, 1.0);
                    let mut path = kurbo::BezPath::new();
                    path.move_to(p0);
                    path.line_to((p0.0 + (p1.0 - p0.0) * f1, p0.1 + (p1.1 - p0.1) * f1));
                    if check_t > 0.45 {
                        let f2 = ((check_t as f64 - 0.45) / 0.55).clamp(0.0, 1.0);
                        path.line_to((p1.0 + (p2.0 - p1.0) * f2, p1.1 + (p2.1 - p1.1) * f2));
                    }
                    scene.stroke_path(path, theme.on_primary.with_alpha(check_t), 2.2);
                }
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(2.0, 2.0),
                        theme.primary.with_alpha(0.75),
                        1.2,
                        CornerRadii::all(6.0),
                    );
                }
                scene.draw_text(
                    Point::new(rect.x + 28.0, rect.y + (rect.height - 16.0) * 0.5),
                    label,
                    TextStyle::default()
                        .with_size(14.0)
                        .with_color(theme.text_primary),
                    None,
                );
            }
            WidgetKind::Switch { checked, label, .. } => {
                let is_on = checked.get_untracked();
                let knob_t = match &ctrl_anim {
                    Some(ControlVisualAnim::Switch { knob_t }) => knob_t.value,
                    _ => {
                        if is_on {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                let track = Rect::new(rect.x, rect.y + (rect.height - 24.0) * 0.5, 44.0, 24.0);
                let track_col = theme
                    .surface_variant
                    .lerp(theme.primary, knob_t.clamp(0.0, 1.0));
                scene.fill_rect(track, track_col, CornerRadii::all(12.0));
                let knob_x = track.x + 3.0 + knob_t * 19.0;
                let knob_r = Rect::new(knob_x, track.y + 3.0, 18.0, 18.0);
                scene.drop_shadow(
                    knob_r,
                    CornerRadii::all(9.0),
                    BoxShadow::drop(0.0, 1.0, 3.0, 0.0, Color::rgba(0, 0, 0, 55)),
                );
                scene.fill_rect(knob_r, Color::WHITE, CornerRadii::all(9.0));
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(2.0, 2.0),
                        theme.primary.with_alpha(0.75),
                        1.2,
                        CornerRadii::all(14.0),
                    );
                }
                if !label.is_empty() {
                    scene.draw_text(
                        Point::new(rect.x + 54.0, rect.y + (rect.height - 16.0) * 0.5),
                        label,
                        TextStyle::default()
                            .with_size(14.0)
                            .with_color(theme.text_primary),
                        None,
                    );
                }
            }
            WidgetKind::Slider {
                value,
                min,
                max,
                label,
                ..
            } => {
                let v = value.get_untracked();
                let raw_frac = if (max - min).abs() > 1e-6 {
                    ((v - min) / (max - min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (frac, hover_t, drag_t) = match &ctrl_anim {
                    Some(ControlVisualAnim::Slider {
                        frac_t,
                        hover_t,
                        drag_t,
                        ..
                    }) => (
                        frac_t.value.clamp(0.0, 1.0),
                        hover_t.value.clamp(0.0, 1.0),
                        drag_t.value.clamp(0.0, 1.0),
                    ),
                    _ => (raw_frac, 0.0, 0.0),
                };
                let cy = if !label.is_empty() && rect.height >= 40.0 {
                    let lbl_style = TextStyle::default()
                        .with_size(13.0)
                        .with_weight(500.0)
                        .with_color(theme.text_secondary);
                    scene.draw_text(
                        Point::new(rect.x + 8.0, rect.y + 2.0),
                        &label,
                        lbl_style,
                        None,
                    );
                    let val_txt = if (v - v.round()).abs() < 0.05 {
                        format!("{:.0}", v)
                    } else {
                        format!("{:.1}", v)
                    };
                    let val_style = TextStyle::default()
                        .with_size(13.0)
                        .with_weight(600.0)
                        .with_color(theme.text_primary);
                    let val_w = self.text_engine.measure(&val_txt, &val_style, None).width;
                    scene.draw_text(
                        Point::new((rect.max_x() - 8.0 - val_w).max(rect.x + 8.0), rect.y + 2.0),
                        val_txt,
                        val_style,
                        None,
                    );
                    rect.max_y() - 12.0
                } else {
                    rect.y + rect.height * 0.5
                };
                let track = Rect::new(rect.x + 8.0, cy - 3.0, (rect.width - 16.0).max(8.0), 6.0);
                scene.fill_rect(track, theme.surface_variant, CornerRadii::all(3.0));
                let active_w = track.width * frac;
                scene.fill_rect(
                    Rect::new(track.x, track.y, active_w, track.height),
                    theme.primary,
                    CornerRadii::all(3.0),
                );
                let kx_center = track.x + active_w;
                if hover_t > 0.01 || drag_t > 0.01 {
                    let halo_r = 9.0 + 4.0 * hover_t.max(drag_t) + 2.0 * drag_t;
                    let halo_alpha = (0.14 * hover_t + 0.18 * drag_t).clamp(0.0, 0.28);
                    scene.fill_rect(
                        Rect::new(kx_center - halo_r, cy - halo_r, halo_r * 2.0, halo_r * 2.0),
                        theme.primary.with_alpha(halo_alpha),
                        CornerRadii::all(halo_r),
                    );
                }
                let thumb_r = 9.0 + 1.5 * drag_t;
                let thumb_rect = Rect::new(
                    kx_center - thumb_r,
                    cy - thumb_r,
                    thumb_r * 2.0,
                    thumb_r * 2.0,
                );
                scene.drop_shadow(
                    thumb_rect,
                    CornerRadii::all(thumb_r),
                    BoxShadow::drop(0.0, 1.5, 4.0, 0.0, Color::rgba(0, 0, 0, 70)),
                );
                scene.fill_rect(thumb_rect, Color::WHITE, CornerRadii::all(thumb_r));
                scene.stroke_rect(
                    thumb_rect,
                    theme.primary,
                    2.0 + 0.5 * drag_t,
                    CornerRadii::all(thumb_r),
                );
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(1.5, 1.5),
                        theme.primary.with_alpha(0.75),
                        1.2,
                        CornerRadii::all(6.0),
                    );
                }
            }
            WidgetKind::Tabs { tabs, active, .. } => {
                let list = tabs.get();
                let count = list.len().max(1);
                let active_idx = active.get_untracked().min(count - 1);
                let anim_idx = match &ctrl_anim {
                    Some(ControlVisualAnim::Tabs { active_t }) => {
                        active_t.value.clamp(0.0, (count - 1) as f32)
                    }
                    _ => active_idx as f32,
                };
                scene.fill_rect(rect, theme.surface_variant, CornerRadii::all(8.0));
                let tab_w = rect.width / count as f32;
                let pill_x = rect.x + anim_idx * tab_w;
                let pill_r = Rect::new(
                    pill_x + 3.0,
                    rect.y + 3.0,
                    (tab_w - 6.0).max(4.0),
                    (rect.height - 6.0).max(4.0),
                );
                scene.fill_rect(pill_r, theme.surface, CornerRadii::all(6.0));
                scene.fill_rect(
                    Rect::new(
                        pill_r.x + 8.0,
                        rect.max_y() - 3.0,
                        (pill_r.width - 16.0).max(4.0),
                        2.5,
                    ),
                    theme.primary,
                    CornerRadii::all(1.25),
                );
                for (i, title) in list.iter().enumerate() {
                    let tx = rect.x + i as f32 * tab_w;
                    let tr = Rect::new(
                        tx + 3.0,
                        rect.y + 3.0,
                        (tab_w - 6.0).max(4.0),
                        rect.height - 6.0,
                    );
                    let act_weight = (1.0 - (anim_idx - i as f32).abs()).clamp(0.0, 1.0);
                    let tst = TextStyle::default()
                        .with_size(14.0)
                        .with_weight(if act_weight > 0.5 { 600.0 } else { 400.0 })
                        .with_color(theme.text_secondary.lerp(theme.text_primary, act_weight));
                    let measured = self.text_engine.measure(title, &tst, None);
                    scene.draw_text(
                        Point::new(
                            tr.x + (tr.width - measured.width).max(0.0) * 0.5,
                            tr.y + (tr.height - measured.height).max(0.0) * 0.5,
                        ),
                        title,
                        tst,
                        Some(tr.width),
                    );
                }
                if is_focused {
                    scene.stroke_rect(rect, theme.primary, 1.5, CornerRadii::all(8.0));
                }
            }
            WidgetKind::Select {
                options,
                selected,
                open,
                dropdown_scroll,
                placeholder,
                ..
            } => {
                let opts = options.get();
                let sel_idx = selected.get_untracked();
                scene.fill_rect(rect, theme.surface, box_style.radius);
                scene.stroke_rect(
                    rect,
                    if is_focused || open {
                        theme.primary
                    } else {
                        theme.border
                    },
                    if is_focused || open { 1.6 } else { 1.0 },
                    box_style.radius,
                );
                let display = opts.get(sel_idx).cloned().unwrap_or(placeholder);
                let is_placeholder = opts.get(sel_idx).is_none();
                scene.draw_text(
                    Point::new(rect.x + 12.0, rect.y + (rect.height - 16.0) * 0.5),
                    display,
                    TextStyle::default()
                        .with_size(14.0)
                        .with_color(if is_placeholder {
                            theme.text_secondary
                        } else {
                            theme.text_primary
                        }),
                    Some((rect.width - 42.0).max(20.0)),
                );
                let chev = Rect::new(
                    rect.max_x() - 28.0,
                    rect.y + (rect.height - 16.0) * 0.5,
                    16.0,
                    16.0,
                );
                paint_svg_icon(
                    scene,
                    chev,
                    if open { "chevron_up" } else { "chevron_down" },
                    theme.text_secondary,
                );
                if open {
                    let screen_trigger = node_xform.transform_rect_bounds(rect);
                    let drop_rect = self.select_dropdown_rect(screen_trigger, opts.len());
                    floating_selects.push((drop_rect, opts, sel_idx, dropdown_scroll));
                }
            }
            WidgetKind::RadioGroup {
                options,
                selected,
                axis,
                ..
            } => {
                let opts = options.get();
                let sel_idx = selected.get_untracked();
                let count = opts.len().max(1);
                let mut cursor_x = rect.x;
                let mut cursor_y = rect.y;
                let horiz_step = rect.width / count as f32;
                for (i, label) in opts.iter().enumerate() {
                    let (ix, iy) = match axis {
                        Axis::Vertical => (rect.x, cursor_y + 4.0),
                        Axis::Horizontal => (cursor_x + 2.0, rect.y + (rect.height - 20.0) * 0.5),
                    };
                    let circle = Rect::new(ix, iy, 18.0, 18.0);
                    let is_sel = i == sel_idx;
                    scene.fill_rect(circle, theme.surface, CornerRadii::all(9.0));
                    scene.stroke_rect(
                        circle,
                        if is_sel { theme.primary } else { theme.border },
                        2.0,
                        CornerRadii::all(9.0),
                    );
                    if is_sel {
                        scene.fill_rect(
                            Rect::new(ix + 4.0, iy + 4.0, 10.0, 10.0),
                            theme.primary,
                            CornerRadii::all(5.0),
                        );
                    }
                    scene.draw_text(
                        Point::new(ix + 26.0, iy + 1.0),
                        label,
                        TextStyle::default()
                            .with_size(14.0)
                            .with_color(theme.text_primary),
                        None,
                    );
                    match axis {
                        Axis::Vertical => cursor_y += 30.0,
                        Axis::Horizontal => cursor_x += horiz_step,
                    }
                }
                if is_focused {
                    scene.stroke_rect(
                        rect.inflate(2.0, 2.0),
                        theme.primary.with_alpha(0.75),
                        1.2,
                        CornerRadii::all(6.0),
                    );
                }
            }
            WidgetKind::SegmentedControl {
                items, selected, ..
            } => {
                let list = items.get();
                let count = list.len().max(1);
                let sel_idx = selected.get_untracked().min(count - 1);
                let anim_idx = match &ctrl_anim {
                    Some(ControlVisualAnim::SegmentedControl { active_t }) => {
                        active_t.value.clamp(0.0, (count - 1) as f32)
                    }
                    _ => sel_idx as f32,
                };
                let track_rad = if box_style.radius == CornerRadii::ZERO {
                    CornerRadii::all(10.0)
                } else {
                    box_style.radius
                };
                if box_style.background.is_none() {
                    scene.fill_rect(rect, theme.surface_variant, track_rad);
                }
                let seg_w = (rect.width - 6.0).max(4.0) / count as f32;
                let pill_r = Rect::new(
                    rect.x + 3.0 + anim_idx * seg_w,
                    rect.y + 3.0,
                    seg_w,
                    (rect.height - 6.0).max(4.0),
                );
                scene.drop_shadow(
                    pill_r,
                    CornerRadii::all(8.0),
                    BoxShadow::drop(0.0, 1.5, 4.0, 0.0, Color::rgba(0, 0, 0, 48)),
                );
                scene.fill_rect(pill_r, theme.surface, CornerRadii::all(8.0));
                scene.stroke_rect(
                    pill_r,
                    theme.border.with_alpha(0.6),
                    1.0,
                    CornerRadii::all(8.0),
                );
                for (i, title) in list.iter().enumerate() {
                    let sr = Rect::new(
                        rect.x + 3.0 + i as f32 * seg_w,
                        rect.y + 3.0,
                        seg_w,
                        (rect.height - 6.0).max(4.0),
                    );
                    let act_weight = (1.0 - (anim_idx - i as f32).abs()).clamp(0.0, 1.0);
                    let tst = TextStyle::default()
                        .with_size(12.5)
                        .with_weight(if act_weight > 0.5 { 600.0 } else { 500.0 })
                        .with_color(theme.text_secondary.lerp(theme.text_primary, act_weight));
                    let max_w = (sr.width - 8.0).max(10.0);
                    let display_title = self.text_engine.truncate_with_ellipsis(title, &tst, max_w);
                    let measured = self.text_engine.measure(&display_title, &tst, None);
                    scene.push_clip(sr, CornerRadii::all(6.0));
                    scene.draw_text(
                        Point::new(
                            sr.x + (sr.width - measured.width).max(0.0) * 0.5,
                            sr.y + (sr.height - measured.height).max(0.0) * 0.5,
                        ),
                        display_title,
                        tst,
                        Some(max_w),
                    );
                    scene.pop_clip();
                }
                if is_focused {
                    scene.stroke_rect(rect.inflate(1.5, 1.5), theme.primary, 1.5, track_rad);
                }
            }
            WidgetKind::BottomNavBar {
                items,
                icons,
                selected,
                ..
            } => {
                let list = items.get();
                let icon_list = icons.get();
                let count = list.len().max(1);
                let sel_idx = selected.get_untracked().min(count - 1);
                let anim_idx = match &ctrl_anim {
                    Some(ControlVisualAnim::BottomNavBar { active_t }) => {
                        active_t.value.clamp(0.0, (count - 1) as f32)
                    }
                    _ => sel_idx as f32,
                };

                let bg_color = match &box_style.background {
                    Some(b) => match b.get() {
                        Brush::Solid(c) => c,
                        _ => theme.surface,
                    },
                    None => theme.surface,
                };
                scene.fill_rect(rect, bg_color, box_style.radius);
                scene.stroke_rect(
                    Rect::new(rect.x, rect.y, rect.width, 1.0),
                    theme.border.with_alpha(0.4),
                    1.0,
                    CornerRadii::ZERO,
                );

                let item_w = rect.width / count as f32;

                // Active indicator pill (Material 3 style: 64x32dp rounded pill)
                let pill_w = (item_w * 0.65).clamp(44.0, 72.0);
                let pill_h = 32.0f32.min(rect.height * 0.55);
                let pill_cx = rect.x + (anim_idx + 0.5) * item_w;
                let pill_cy = rect.y + 18.0;
                let pill_r = Rect::new(
                    pill_cx - pill_w * 0.5,
                    pill_cy - pill_h * 0.5,
                    pill_w,
                    pill_h,
                );

                let pill_bg = theme.primary.with_alpha(0.18);
                scene.fill_rect(pill_r, pill_bg, CornerRadii::all(16.0));

                for (i, title) in list.iter().enumerate() {
                    let cx = rect.x + (i as f32 + 0.5) * item_w;
                    let act_dist = (anim_idx - i as f32).abs();
                    let act_weight = (1.0 - act_dist).clamp(0.0, 1.0);

                    let icon_glyph = icon_list.get(i).map(|s| s.as_str()).unwrap_or("");
                    let icon_color = theme.text_secondary.lerp(theme.primary, act_weight);

                    if !icon_glyph.is_empty() {
                        let icon_st = TextStyle::default().with_size(18.0).with_color(icon_color);
                        let isz = self.text_engine.measure(icon_glyph, &icon_st, None);
                        scene.draw_text(
                            Point::new(cx - isz.width * 0.5, rect.y + 9.0),
                            icon_glyph,
                            icon_st,
                            None,
                        );
                    }

                    let lbl_st = TextStyle::default()
                        .with_size(11.5)
                        .with_weight(if act_weight > 0.5 { 600.0 } else { 500.0 })
                        .with_color(theme.text_secondary.lerp(theme.text_primary, act_weight));
                    let max_lbl_w = (item_w - 6.0).max(10.0);
                    let display_lbl = self
                        .text_engine
                        .truncate_with_ellipsis(title, &lbl_st, max_lbl_w);
                    let lsz = self.text_engine.measure(&display_lbl, &lbl_st, None);

                    let item_rect =
                        Rect::new(rect.x + i as f32 * item_w, rect.y, item_w, rect.height);
                    scene.push_clip(item_rect, CornerRadii::ZERO);
                    scene.draw_text(
                        Point::new(
                            cx - lsz.width * 0.5,
                            rect.y + rect.height - lsz.height - 6.0,
                        ),
                        display_lbl,
                        lbl_st,
                        Some(max_lbl_w),
                    );
                    scene.pop_clip();
                }
            }
            WidgetKind::FloatingActionButton {
                icon,
                label,
                disabled,
                foreground,
                background,
                hovered,
                pressed,
                ..
            } => {
                let is_dis = is_disabled || disabled.get();
                let (hover_t, press_t) = match &ctrl_anim {
                    Some(ControlVisualAnim::FloatingActionButton { hover_t, press_t }) => {
                        (hover_t.value.clamp(0.0, 1.0), press_t.value.clamp(0.0, 1.0))
                    }
                    _ => (
                        if hovered { 1.0 } else { 0.0 },
                        if pressed { 1.0 } else { 0.0 },
                    ),
                };
                let bg = if is_dis {
                    theme.surface_variant
                } else if let Some(bg_rv) = background {
                    bg_rv.get()
                } else {
                    theme.primary
                };
                let fg = if is_dis {
                    theme.text_secondary
                } else if let Some(fg_rv) = foreground {
                    fg_rv.get()
                } else {
                    Color::WHITE
                };

                let rad = CornerRadii::all(rect.height * 0.5);
                let shadow_blur = 10.0 + hover_t * 6.0 - press_t * 3.0;
                let shadow_y = 3.5 + hover_t * 2.0 - press_t * 1.5;
                scene.drop_shadow(
                    rect,
                    rad,
                    BoxShadow::drop(0.0, shadow_y, shadow_blur, 0.0, Color::rgba(0, 0, 0, 70)),
                );

                scene.fill_rect(rect, bg, rad);
                if hover_t > 0.01 {
                    scene.fill_rect(
                        rect,
                        Color::rgba(255, 255, 255, (hover_t * 30.0) as u8),
                        rad,
                    );
                }

                let icon_str = icon.get();
                let label_str = label.as_ref().map(|l| l.get());

                if let Some(lbl) = label_str {
                    let icon_st = TextStyle::default().with_size(18.0).with_color(fg);
                    let lbl_st = TextStyle::default()
                        .with_size(14.0)
                        .with_weight(600.0)
                        .with_color(fg);
                    let isz = self.text_engine.measure(&icon_str, &icon_st, None);
                    let lsz = self.text_engine.measure(&lbl, &lbl_st, None);
                    let gap = 8.0;
                    let total_w = isz.width + gap + lsz.width;
                    let start_x = rect.x + (rect.width - total_w).max(0.0) * 0.5;

                    scene.draw_text(
                        Point::new(start_x, rect.y + (rect.height - isz.height) * 0.5),
                        icon_str,
                        icon_st,
                        None,
                    );
                    scene.draw_text(
                        Point::new(
                            start_x + isz.width + gap,
                            rect.y + (rect.height - lsz.height) * 0.5,
                        ),
                        lbl,
                        lbl_st,
                        None,
                    );
                } else {
                    let icon_st = TextStyle::default().with_size(24.0).with_color(fg);
                    let isz = self.text_engine.measure(&icon_str, &icon_st, None);
                    scene.draw_text(
                        Point::new(
                            rect.x + (rect.width - isz.width) * 0.5,
                            rect.y + (rect.height - isz.height) * 0.5,
                        ),
                        icon_str,
                        icon_st,
                        None,
                    );
                }
            }
            WidgetKind::Accordion {
                title, expanded, ..
            } => {
                let is_exp = expanded.get_untracked();
                let card_rad = if box_style.radius == CornerRadii::ZERO {
                    CornerRadii::all(10.0)
                } else {
                    box_style.radius
                };
                if box_style.background.is_none() {
                    scene.fill_rect(rect, theme.surface, card_rad);
                }
                if box_style.border.is_none() {
                    scene.stroke_rect(rect, theme.border, 1.0, card_rad);
                }
                let header_h = 44.0f32.min(rect.height);
                let header_r = Rect::new(rect.x, rect.y, rect.width, header_h);
                if is_exp {
                    scene.fill_rect(
                        Rect::new(
                            rect.x + 1.0,
                            rect.y + 1.0,
                            (rect.width - 2.0).max(0.0),
                            (header_h - 2.0).max(0.0),
                        ),
                        theme.surface_variant.with_alpha(0.32),
                        CornerRadii::all(8.0),
                    );
                    scene.fill_rect(
                        Rect::new(
                            rect.x + 12.0,
                            rect.y + header_h - 1.0,
                            (rect.width - 24.0).max(0.0),
                            1.0,
                        ),
                        theme.border,
                        CornerRadii::ZERO,
                    );
                }
                let t_str = title.get();
                let tst = TextStyle::default()
                    .with_size(14.0)
                    .with_weight(600.0)
                    .with_color(theme.text_primary);
                let measured = self.text_engine.measure(&t_str, &tst, None);
                scene.draw_text(
                    Point::new(
                        rect.x + 14.0,
                        rect.y + (header_h - measured.height).max(0.0) * 0.5,
                    ),
                    t_str,
                    tst,
                    Some((rect.width - 52.0).max(20.0)),
                );
                let chev_r = Rect::new(
                    rect.max_x() - 28.0,
                    rect.y + (header_h - 16.0) * 0.5,
                    16.0,
                    16.0,
                );
                let _ = paint_svg_icon(
                    scene,
                    chev_r,
                    if is_exp { "chevron_up" } else { "chevron_down" },
                    theme.text_secondary,
                );
                if is_focused {
                    scene.stroke_rect(header_r, theme.primary, 1.6, card_rad);
                }
                paint_children = is_exp;
            }
            WidgetKind::DataTable {
                columns,
                rows,
                selected,
                sort_column,
                sort_ascending,
                scroll_offset,
                striped,
                row_height,
                ..
            } => {
                let cols = columns.get();
                let row_list = rows.get();
                let tbl_rad = if box_style.radius == CornerRadii::ZERO {
                    CornerRadii::all(8.0)
                } else {
                    box_style.radius
                };
                if box_style.background.is_none() {
                    scene.fill_rect(rect, theme.surface, tbl_rad);
                }
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: tbl_rad,
                });
                let header_h = 38.0f32.min(rect.height);
                let header_r = Rect::new(rect.x, rect.y, rect.width, header_h);
                scene.fill_rect(header_r, theme.surface_variant, CornerRadii::ZERO);
                scene.fill_rect(
                    Rect::new(rect.x, rect.y + header_h - 1.0, rect.width, 1.0),
                    theme.border,
                    CornerRadii::ZERO,
                );
                let col_count = cols.len().max(1);
                let col_w = (rect.width / col_count as f32).max(40.0);
                for (ci, col_title) in cols.iter().enumerate() {
                    let cx = rect.x + ci as f32 * col_w;
                    let is_sorted = sort_column == Some(ci);
                    let hst = TextStyle::default()
                        .with_size(12.5)
                        .with_weight(600.0)
                        .with_color(if is_sorted {
                            theme.primary
                        } else {
                            theme.text_secondary
                        });
                    let measured = self.text_engine.measure(col_title, &hst, None);
                    scene.draw_text(
                        Point::new(
                            cx + 12.0,
                            rect.y + (header_h - measured.height).max(0.0) * 0.5,
                        ),
                        col_title,
                        hst,
                        Some((col_w - if is_sorted { 34.0 } else { 20.0 }).max(16.0)),
                    );
                    if is_sorted {
                        let chev = Rect::new(
                            cx + col_w - 20.0,
                            rect.y + (header_h - 12.0) * 0.5,
                            12.0,
                            12.0,
                        );
                        let _ = paint_svg_icon(
                            scene,
                            chev,
                            if sort_ascending {
                                "chevron_up"
                            } else {
                                "chevron_down"
                            },
                            theme.primary,
                        );
                    }
                }
                let body_rect = Rect::new(
                    rect.x,
                    rect.y + header_h,
                    rect.width,
                    (rect.height - header_h).max(0.0),
                );
                scene.push(PaintCommand::PushClip {
                    rect: body_rect,
                    radius: CornerRadii::ZERO,
                });
                let sorted =
                    Self::data_table_sorted_indices(&row_list, sort_column, sort_ascending);
                let total_h = sorted.len() as f32 * row_height;
                let max_scroll = (total_h - body_rect.height).max(0.0);
                let scroll = scroll_offset.clamp(0.0, max_scroll);
                let sel_orig = selected.get_untracked();
                for (vis_i, &orig_i) in sorted.iter().enumerate() {
                    let ry = body_rect.y + vis_i as f32 * row_height - scroll;
                    if ry + row_height < body_rect.y || ry > body_rect.max_y() {
                        continue;
                    }
                    let row_r = Rect::new(body_rect.x, ry, body_rect.width, row_height);
                    let is_sel = orig_i == sel_orig;
                    if is_sel {
                        scene.fill_rect(row_r, theme.primary.with_alpha(0.16), CornerRadii::ZERO);
                    } else if striped && vis_i % 2 == 1 {
                        scene.fill_rect(
                            row_r,
                            theme.surface_variant.with_alpha(0.32),
                            CornerRadii::ZERO,
                        );
                    }
                    scene.fill_rect(
                        Rect::new(row_r.x, row_r.max_y() - 1.0, row_r.width, 1.0),
                        theme.border.with_alpha(0.45),
                        CornerRadii::ZERO,
                    );
                    if let Some(cells) = row_list.get(orig_i) {
                        for (ci, cell) in cells.iter().take(col_count).enumerate() {
                            let cx = rect.x + ci as f32 * col_w;
                            let cst = TextStyle::default()
                                .with_size(13.0)
                                .with_weight(if is_sel { 500.0 } else { 400.0 })
                                .with_color(if is_sel {
                                    theme.primary
                                } else {
                                    theme.text_primary
                                });
                            let measured = self.text_engine.measure(cell, &cst, None);
                            scene.draw_text(
                                Point::new(
                                    cx + 12.0,
                                    row_r.y + (row_r.height - measured.height).max(0.0) * 0.5,
                                ),
                                cell,
                                cst,
                                Some((col_w - 20.0).max(16.0)),
                            );
                        }
                    }
                }
                if max_scroll > 0.5 && body_rect.height > 8.0 {
                    let ratio = (body_rect.height / total_h).clamp(0.12, 1.0);
                    let track_h = (body_rect.height - 8.0).max(8.0);
                    let thumb_h = (track_h * ratio).max(16.0);
                    let pos_ratio = (scroll / max_scroll).clamp(0.0, 1.0);
                    let thumb_y = body_rect.y + 4.0 + (track_h - thumb_h) * pos_ratio;
                    scene.fill_rect(
                        Rect::new(body_rect.max_x() - 6.0, thumb_y, 3.5, thumb_h),
                        theme.text_secondary.with_alpha(0.55),
                        CornerRadii::all(2.0),
                    );
                }
                scene.push(PaintCommand::PopClip);
                scene.push(PaintCommand::PopClip);
                if box_style.border.is_none() {
                    scene.stroke_rect(rect, theme.border, 1.0, tbl_rad);
                }
                if is_focused {
                    scene.stroke_rect(rect.inflate(1.5, 1.5), theme.primary, 1.5, tbl_rad);
                }
            }
            WidgetKind::PopupMenu { items, anchor, .. } => {
                let reveal = match &ctrl_anim {
                    Some(ControlVisualAnim::Overlay { reveal, .. }) => reveal.value.clamp(0.0, 1.2),
                    _ => 1.0,
                };
                let reveal_op = reveal.clamp(0.0, 1.0);
                let list = items.get();
                let menu_r = self.popup_menu_rect(anchor, list.len(), rect);
                scene.fill_rect(
                    rect,
                    Color::rgba(0, 0, 0, (60.0 * reveal_op).round() as u8),
                    CornerRadii::ZERO,
                );
                let animate_menu = reveal_op < 0.999;
                if animate_menu {
                    let s = (0.94 + 0.06 * reveal).clamp(0.85, 1.04);
                    scene.push_transform(Transform2D::around_pivot(
                        menu_r.center(),
                        Transform2D::scale(s, s),
                    ));
                    scene.push_layer(menu_r.inflate(28.0, 28.0), reveal_op, BlendMode::Normal);
                }
                scene.drop_shadow(
                    menu_r,
                    CornerRadii::all(8.0),
                    BoxShadow::drop(0.0, 6.0, 14.0, 0.0, Color::rgba(0, 0, 0, 120)),
                );
                scene.fill_rect(menu_r, theme.surface, CornerRadii::all(8.0));
                scene.stroke_rect(menu_r, theme.border, 1.0, CornerRadii::all(8.0));
                for (i, label) in list.iter().enumerate() {
                    let iy = menu_r.y + 4.0 + i as f32 * 36.0;
                    if iy + 34.0 > menu_r.max_y() {
                        break;
                    }
                    scene.draw_text(
                        Point::new(menu_r.x + 14.0, iy + 9.0),
                        label,
                        TextStyle::default()
                            .with_size(14.0)
                            .with_color(theme.text_primary),
                        Some((menu_r.width - 28.0).max(20.0)),
                    );
                }
                if animate_menu {
                    scene.pop_layer();
                    scene.pop_transform();
                }
            }
            WidgetKind::Tooltip { text } => {
                if self.hovered_has_ancestor(id) {
                    let screen_r = node_xform.transform_rect_bounds(rect);
                    active_tooltips.push((screen_r, text.get()));
                }
            }
            WidgetKind::ProgressIndicator { progress, phase } => {
                scene.fill_rect(
                    rect,
                    theme.surface_variant,
                    CornerRadii::all(rect.height * 0.5),
                );
                match progress.get() {
                    Some(p) => {
                        let w = rect.width * p.clamp(0.0, 1.0);
                        scene.fill_rect(
                            Rect::new(rect.x, rect.y, w, rect.height),
                            theme.primary,
                            CornerRadii::all(rect.height * 0.5),
                        );
                    }
                    None => {
                        let bar_w = rect.width * 0.35;
                        let start_x = rect.x + (rect.width - bar_w) * phase;
                        scene.fill_rect(
                            Rect::new(start_x, rect.y, bar_w, rect.height),
                            theme.primary,
                            CornerRadii::all(rect.height * 0.5),
                        );
                    }
                }
            }
            WidgetKind::Spinner {
                size,
                thickness,
                color,
                phase,
            } => {
                use kurbo::Shape as _;
                let dia = rect
                    .width
                    .min(rect.height)
                    .max(size.min(rect.width.max(rect.height)));
                let stroke_w = thickness.clamp(1.5, (dia * 0.35).max(1.5));
                let radius = ((dia - stroke_w) * 0.5).max(2.0) as f64;
                let center = rect.center();
                let c = (center.x as f64, center.y as f64);
                let track_path = kurbo::Circle::new(c, radius).to_path(0.1);
                scene.stroke_path(track_path, theme.surface_variant, stroke_w);
                let start_angle =
                    (phase as f64) * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
                let sweep = std::f64::consts::PI * 1.5;
                let arc_path =
                    kurbo::Arc::new(c, (radius, radius), start_angle, sweep, 0.0).to_path(0.1);
                let fg = color.map(|cv| cv.get()).unwrap_or(theme.primary);
                scene.stroke_path(arc_path, fg, stroke_w);
            }
            WidgetKind::Badge {
                label,
                foreground,
                background,
                icon,
            } => {
                let pill_radius = if box_style.radius == CornerRadii::ZERO
                    || box_style.radius == CornerRadii::all(12.0)
                {
                    CornerRadii::all(rect.height * 0.5)
                } else {
                    box_style.radius
                };
                let fg = foreground.map(|f| f.get()).unwrap_or(theme.primary);
                let bg = if let Some(b) = background {
                    b.get()
                } else if box_style.background.is_none() {
                    fg.with_alpha(0.16)
                } else {
                    Color::TRANSPARENT
                };
                if bg != Color::TRANSPARENT {
                    scene.fill_rect(rect, bg, pill_radius);
                }
                if box_style.border.is_none() {
                    scene.stroke_rect(rect, fg.with_alpha(0.32), 1.0, pill_radius);
                }
                let txt = label.get();
                let tstyle = TextStyle::default()
                    .with_size(12.0)
                    .with_weight(600.0)
                    .with_color(fg);
                let measured = self.text_engine.measure(&txt, &tstyle, None);
                let icon_spec = icon.map(|iv| iv.get()).filter(|s| !s.is_empty());
                let icon_total = if icon_spec.is_some() { 16.0 } else { 0.0 };
                let content_w = icon_total + measured.width;
                let start_x = rect.x + (rect.width - content_w).max(0.0) * 0.5;
                if let Some(ref ic) = icon_spec {
                    let ir = Rect::new(start_x, rect.y + (rect.height - 12.0) * 0.5, 12.0, 12.0);
                    let _ = paint_svg_icon(scene, ir, ic, fg);
                }
                let tx = start_x + icon_total;
                let ty = rect.y + (rect.height - measured.height) * 0.5;
                scene.draw_text(Point::new(tx, ty), txt, tstyle, None);
            }
            WidgetKind::SplitPane {
                axis,
                ratio,
                min_first,
                min_second,
                divider_size,
                dragging,
                ..
            } => {
                let inner = rect.inset(node_padding);
                let (_, _, div_rect) = Self::split_divider_geometry(
                    inner,
                    axis,
                    ratio.get_untracked(),
                    min_first,
                    min_second,
                    divider_size,
                );
                let track_bg = if dragging {
                    theme.primary.with_alpha(0.18)
                } else {
                    theme.surface_variant.with_alpha(0.65)
                };
                scene.fill_rect(div_rect, track_bg, CornerRadii::ZERO);
                let handle = match axis {
                    Axis::Horizontal => Rect::new(
                        div_rect.center().x - 1.5,
                        div_rect.center().y - 14.0,
                        3.0,
                        28.0,
                    ),
                    Axis::Vertical => Rect::new(
                        div_rect.center().x - 14.0,
                        div_rect.center().y - 1.5,
                        28.0,
                        3.0,
                    ),
                };
                let handle_col = if dragging {
                    theme.primary
                } else {
                    theme.text_secondary.with_alpha(0.55)
                };
                scene.fill_rect(handle, handle_col, CornerRadii::all(1.5));
            }
            WidgetKind::ToastHost { items, .. } => {
                let list = items.get();
                let (cards, closes) = Self::toast_card_rects(rect, list.len());
                for ((item, card_r), close_r) in list.iter().zip(cards).zip(closes) {
                    if card_r.max_y() < rect.y {
                        continue;
                    }
                    let (accent, icon_name) = match item.severity {
                        ToastSeverity::Info => (theme.primary, "info"),
                        ToastSeverity::Success => (Color::rgb(16, 185, 129), "check"),
                        ToastSeverity::Warning => (Color::rgb(245, 158, 11), "warning"),
                        ToastSeverity::Error => (theme.danger, "warning"),
                    };
                    scene.drop_shadow(
                        card_r,
                        CornerRadii::all(10.0),
                        BoxShadow::drop(0.0, 6.0, 16.0, 0.0, Color::rgba(0, 0, 0, 110)),
                    );
                    scene.fill_rect(card_r, theme.surface, CornerRadii::all(10.0));
                    scene.stroke_rect(card_r, theme.border, 1.0, CornerRadii::all(10.0));
                    scene.fill_rect(
                        Rect::new(card_r.x + 6.0, card_r.y + 10.0, 4.0, card_r.height - 20.0),
                        accent,
                        CornerRadii::all(2.0),
                    );
                    let _ = paint_svg_icon(
                        scene,
                        Rect::new(card_r.x + 17.0, card_r.y + 14.0, 16.0, 16.0),
                        icon_name,
                        accent,
                    );
                    let max_text_w = (card_r.width - 78.0).max(40.0);
                    scene.draw_text(
                        Point::new(card_r.x + 41.0, card_r.y + 11.0),
                        &item.title,
                        TextStyle::default()
                            .with_size(13.0)
                            .with_weight(600.0)
                            .with_color(theme.text_primary),
                        Some(max_text_w),
                    );
                    scene.draw_text(
                        Point::new(card_r.x + 41.0, card_r.y + 32.0),
                        &item.message,
                        TextStyle::default()
                            .with_size(12.0)
                            .with_color(theme.text_secondary),
                        Some(max_text_w),
                    );
                    let _ = paint_svg_icon(
                        scene,
                        close_r.inset(Insets::all(3.0)),
                        "close",
                        theme.text_secondary,
                    );
                }
            }
            WidgetKind::Overlay {
                is_bottom_sheet, ..
            } => {
                let reveal = match &ctrl_anim {
                    Some(ControlVisualAnim::Overlay { reveal, .. }) => reveal.value.clamp(0.0, 1.2),
                    _ => 1.0,
                };
                let reveal_op = reveal.clamp(0.0, 1.0);
                scene.fill_rect(
                    rect,
                    Color::rgba(0, 0, 0, (145.0 * reveal_op).round() as u8),
                    CornerRadii::ZERO,
                );
                if reveal_op < 0.999 {
                    let overlay_xf = if is_bottom_sheet {
                        Transform2D::translation(0.0, (1.0 - reveal) * 44.0)
                    } else {
                        let s = (0.94 + 0.06 * reveal).clamp(0.85, 1.04);
                        Transform2D::around_pivot(rect.center(), Transform2D::scale(s, s))
                    };
                    extra_overlay_anim = Some((overlay_xf, reveal_op));
                }
            }
            WidgetKind::PlatformView { id: pv_id } => {
                scene.push(PaintCommand::PlatformViewHole { id: pv_id, rect });
            }
            WidgetKind::HtmlView { .. } => {
                if box_style.background.is_none() {
                    scene.fill_rect(rect, theme.surface, box_style.radius);
                }
                scene.push(PaintCommand::PlatformViewHole {
                    id: PlatformViewId::from_raw(rect.x as u64 ^ rect.y as u64),
                    rect,
                });
            }
            WidgetKind::Card { variant } => {
                let radius = if box_style.radius == CornerRadii::ZERO {
                    CornerRadii::all(12.0)
                } else {
                    box_style.radius
                };
                match variant {
                    CardVariant::Elevated => {
                        scene.drop_shadow(
                            rect,
                            radius,
                            BoxShadow::drop(0.0, 1.5, 4.0, 0.0, Color::rgba(0, 0, 0, 45)),
                        );
                        let bg = box_style
                            .background
                            .as_ref()
                            .map(|b| b.get())
                            .unwrap_or(Brush::Solid(theme.surface));
                        let adapted = Self::adapt_brush_to_rect(&bg, rect);
                        scene.fill_rect(rect, adapted, radius);
                    }
                    CardVariant::Filled => {
                        let bg = box_style
                            .background
                            .as_ref()
                            .map(|b| b.get())
                            .unwrap_or(Brush::Solid(theme.surface_variant));
                        let adapted = Self::adapt_brush_to_rect(&bg, rect);
                        scene.fill_rect(rect, adapted, radius);
                    }
                    CardVariant::Outlined => {
                        let bg = box_style
                            .background
                            .as_ref()
                            .map(|b| b.get())
                            .unwrap_or(Brush::Solid(theme.surface));
                        let adapted = Self::adapt_brush_to_rect(&bg, rect);
                        scene.fill_rect(rect, adapted, radius);
                        let border_c = box_style
                            .border_color
                            .as_ref()
                            .map(|c| c.get())
                            .unwrap_or(theme.border);
                        scene.stroke_rect(rect, border_c, 1.0, radius);
                    }
                }
            }
            WidgetKind::ListTile {
                leading,
                title,
                subtitle,
                trailing,
                hovered,
                pressed,
                selected,
                ..
            } => {
                let is_sel = selected.get();
                let bg = if pressed {
                    Some(theme.primary.with_alpha(0.18))
                } else if hovered {
                    Some(theme.surface_variant.with_alpha(0.6))
                } else if is_sel {
                    Some(theme.primary.with_alpha(0.10))
                } else {
                    None
                };
                if let Some(c) = bg {
                    scene.fill_rect(rect, c, box_style.radius);
                }

                let pad_x = 16.0;
                let mut cur_x = rect.x + pad_x;

                if let Some(lead) = leading {
                    let lead_str = lead.get();
                    if !lead_str.is_empty() {
                        let tint = if is_sel {
                            theme.primary
                        } else {
                            theme.text_secondary
                        };
                        if builtin_icon_svg(&lead_str).is_some() {
                            let icon_sz = 20.0;
                            let icon_y = rect.y + (rect.height - icon_sz) * 0.5;
                            let icon_rect = Rect::new(cur_x, icon_y, icon_sz, icon_sz);
                            let _ = paint_svg_icon(scene, icon_rect, &lead_str, tint);
                            cur_x += icon_sz + 16.0;
                        } else {
                            let lead_st = TextStyle::default().with_size(20.0).with_color(tint);
                            let lsz = self.text_engine.measure(&lead_str, &lead_st, None);
                            let icon_y = rect.y + (rect.height - lsz.height) * 0.5;
                            scene.draw_text(Point::new(cur_x, icon_y), lead_str, lead_st, None);
                            cur_x += lsz.width + 16.0;
                        }
                    }
                }

                let mut trailing_w = 0.0;
                if let Some(trail) = trailing {
                    let trail_str = trail.get();
                    if !trail_str.is_empty() {
                        let trail_st = TextStyle::default()
                            .with_size(18.0)
                            .with_color(theme.text_secondary);
                        let tsz = self.text_engine.measure(&trail_str, &trail_st, None);
                        trailing_w = tsz.width + 16.0;
                        let trail_x = rect.x + rect.width - pad_x - tsz.width;
                        let trail_y = rect.y + (rect.height - tsz.height) * 0.5;
                        scene.draw_text(Point::new(trail_x, trail_y), trail_str, trail_st, None);
                    }
                }

                let title_str = title.get();
                let avail_w = (rect.x + rect.width - cur_x - trailing_w - pad_x).max(10.0);
                let title_col = if is_sel {
                    theme.primary
                } else {
                    theme.text_primary
                };
                let title_st = TextStyle::default()
                    .with_size(15.0)
                    .with_weight(500.0)
                    .with_color(title_col);

                if let Some(sub) = subtitle {
                    let sub_str = sub.get();
                    let sub_st = TextStyle::default()
                        .with_size(13.0)
                        .with_color(theme.text_secondary);
                    let t_sz = self
                        .text_engine
                        .measure(&title_str, &title_st, Some(avail_w));
                    let s_sz = self.text_engine.measure(&sub_str, &sub_st, Some(avail_w));
                    let total_h = t_sz.height + s_sz.height + 2.0;
                    let start_y = rect.y + (rect.height - total_h) * 0.5;

                    scene.draw_text(
                        Point::new(cur_x, start_y),
                        title_str,
                        title_st,
                        Some(avail_w),
                    );
                    scene.draw_text(
                        Point::new(cur_x, start_y + t_sz.height + 2.0),
                        sub_str,
                        sub_st,
                        Some(avail_w),
                    );
                } else {
                    let t_sz = self
                        .text_engine
                        .measure(&title_str, &title_st, Some(avail_w));
                    let start_y = rect.y + (rect.height - t_sz.height) * 0.5;
                    scene.draw_text(
                        Point::new(cur_x, start_y),
                        title_str,
                        title_st,
                        Some(avail_w),
                    );
                }
            }
            WidgetKind::Chip {
                label,
                icon,
                selected,
                hovered,
                ..
            } => {
                let is_sel = selected.get();
                let rad = CornerRadii::all(8.0);
                let bg = if is_sel {
                    theme.primary.with_alpha(0.2)
                } else if hovered {
                    theme.surface_variant.with_alpha(0.6)
                } else {
                    theme.surface
                };
                scene.fill_rect(rect, bg, rad);
                let border_color = if is_sel { theme.primary } else { theme.border };
                scene.stroke_rect(rect, border_color, 1.0, rad);

                let mut cur_x = rect.x + 10.0;
                if is_sel && icon.is_none() {
                    let check_st = TextStyle::default()
                        .with_size(14.0)
                        .with_color(theme.primary);
                    let csz = self.text_engine.measure("✓", &check_st, None);
                    let cy = rect.y + (rect.height - csz.height) * 0.5;
                    scene.draw_text(Point::new(cur_x, cy), "✓", check_st, None);
                    cur_x += csz.width + 6.0;
                } else if let Some(ic) = icon {
                    let ic_str = ic.get();
                    if !ic_str.is_empty() {
                        let ic_st = TextStyle::default().with_size(14.0).with_color(if is_sel {
                            theme.primary
                        } else {
                            theme.text_secondary
                        });
                        let isz = self.text_engine.measure(&ic_str, &ic_st, None);
                        let iy = rect.y + (rect.height - isz.height) * 0.5;
                        scene.draw_text(Point::new(cur_x, iy), ic_str, ic_st, None);
                        cur_x += isz.width + 6.0;
                    }
                }

                let lbl_str = label.get();
                let lbl_col = if is_sel {
                    theme.primary
                } else {
                    theme.text_primary
                };
                let lbl_st = TextStyle::default()
                    .with_size(13.0)
                    .with_weight(500.0)
                    .with_color(lbl_col);
                let lsz = self.text_engine.measure(&lbl_str, &lbl_st, None);
                let ly = rect.y + (rect.height - lsz.height) * 0.5;
                scene.draw_text(Point::new(cur_x, ly), lbl_str, lbl_st, None);
            }
            WidgetKind::AppBar {
                title,
                leading_icon,
                elevation,
                ..
            } => {
                let bg = box_style
                    .background
                    .as_ref()
                    .map(|b| b.get())
                    .unwrap_or(Brush::Solid(theme.surface));
                let adapted = Self::adapt_brush_to_rect(&bg, rect);
                scene.fill_rect(rect, adapted, CornerRadii::ZERO);

                if elevation > 0.0 {
                    scene.drop_shadow(
                        rect,
                        CornerRadii::ZERO,
                        BoxShadow::drop(
                            0.0,
                            elevation * 1.5,
                            elevation * 4.0,
                            0.0,
                            Color::rgba(0, 0, 0, 45),
                        ),
                    );
                }

                let mut cur_x = rect.x + 16.0;
                if let Some(lead) = leading_icon {
                    let lead_str = lead.get();
                    if !lead_str.is_empty() {
                        let lead_st = TextStyle::default()
                            .with_size(22.0)
                            .with_color(theme.text_primary);
                        let lsz = self.text_engine.measure(&lead_str, &lead_st, None);
                        let ly = rect.y + (rect.height - lsz.height) * 0.5;
                        scene.draw_text(Point::new(cur_x, ly), lead_str, lead_st, None);
                        cur_x += lsz.width + 16.0;
                    }
                }

                let title_str = title.get();
                let title_st = TextStyle::default()
                    .with_size(18.0)
                    .with_weight(600.0)
                    .with_color(theme.text_primary);
                let tsz = self.text_engine.measure(&title_str, &title_st, None);
                let ty = rect.y + (rect.height - tsz.height) * 0.5;
                scene.draw_text(Point::new(cur_x, ty), title_str, title_st, None);
            }
            WidgetKind::Drawer { open, width, .. } => {
                if !open.get() {
                    paint_children = false;
                } else {
                    scene.fill_rect(rect, Color::rgba(0, 0, 0, 120), CornerRadii::ZERO);

                    let drawer_w = width.min(rect.width * 0.85).max(240.0);
                    let drawer_r = Rect::new(rect.x, rect.y, drawer_w, rect.height);

                    scene.drop_shadow(
                        drawer_r,
                        CornerRadii::new(0.0, 16.0, 16.0, 0.0),
                        BoxShadow::drop(4.0, 0.0, 16.0, 0.0, Color::rgba(0, 0, 0, 60)),
                    );

                    let bg = box_style
                        .background
                        .as_ref()
                        .map(|b| b.get())
                        .unwrap_or(Brush::Solid(theme.surface));
                    let adapted = Self::adapt_brush_to_rect(&bg, drawer_r);
                    scene.fill_rect(drawer_r, adapted, CornerRadii::new(0.0, 16.0, 16.0, 0.0));
                }
            }
            WidgetKind::Custom(cw) => {
                if let Ok(mut guard) = cw.try_borrow_mut() {
                    guard.paint(rect, scene, &mut self.text_engine, theme, is_focused);
                }
            }
        }

        let eff_border_col =
            anim_border.or_else(|| box_style.border_color.as_ref().map(ReactiveValue::get));
        if let Some(border) = box_style.border
            && border.width > 0.0
        {
            let bcol = eff_border_col.unwrap_or(border.color);
            scene.stroke_rect(rect, bcol, border.width, border.radius);
        }

        if paint_children {
            if box_style.clip {
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
            }
            let (child_xf_for_sub, pushed_overlay_anim) =
                if let Some((ov_xf, ov_op)) = extra_overlay_anim {
                    let mut child_bounds = rect;
                    for &child in &children {
                        self.accumulate_subtree_bounds(
                            child,
                            child_offset,
                            Transform2D::IDENTITY,
                            &mut child_bounds,
                        );
                    }
                    scene.push_transform(ov_xf);
                    scene.push_layer(child_bounds, ov_op, BlendMode::Normal);
                    (node_xform * ov_xf, true)
                } else {
                    (node_xform, false)
                };
            for child in children {
                self.paint_node_recursive(
                    child,
                    child_offset,
                    child_xf_for_sub,
                    theme,
                    scene,
                    floating_selects,
                    active_tooltips,
                );
            }
            if pushed_overlay_anim {
                scene.pop_layer();
                scene.pop_transform();
            }
            if box_style.clip {
                scene.push(PaintCommand::PopClip);
            }
        }

        if needs_layer {
            scene.pop_layer();
        }
        if has_xform {
            scene.pop_transform();
        }

        if let Some(n) = self.nodes.get_mut(id) {
            n.dirty = DirtyFlags::NONE;
        }
    }

    // === Event Routing & Hit-Testing ===

    fn node_world_transform(&self, id: NodeId) -> Transform2D {
        let mut chain = Vec::new();
        let mut cur = Some(id);
        while let Some(cid) = cur {
            chain.push(cid);
            cur = self.nodes.get(cid).and_then(|n| n.parent);
        }
        let mut world = Transform2D::IDENTITY;
        for aid in chain.into_iter().rev() {
            let r = self.node_visual_rect(aid);
            let local_xf = self.node_effective_transform(aid, r.center());
            if !local_xf.is_identity() {
                world = world * local_xf;
            }
        }
        world
    }

    fn screen_to_node_local_point(&self, id: NodeId, point: Point) -> Point {
        let world = self.node_world_transform(id);
        if world.is_identity() {
            point
        } else {
            world
                .inverse()
                .map_or(point, |inv| inv.transform_point(point))
        }
    }

    /// Hit-test the deepest interactive/visible node under `point`, respecting open dropdowns,
    /// active transforms, scroll offsets, and `hit_slop`.
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        let root = self.root?;

        // Open Select dropdowns capture hits above normal tree nodes in topmost paint order
        let mut order = Vec::new();
        self.collect_preorder(root, &mut order);
        for id in order.into_iter().rev() {
            if let Some(node) = self.nodes.get(id)
                && !node.layout.hidden
                && let WidgetKind::Select {
                    open: true,
                    options,
                    ..
                } = &node.kind
            {
                let tr = self.node_visual_rect(id);
                let screen_tr = self.node_world_transform(id).transform_rect_bounds(tr);
                let dr = self.select_dropdown_rect(screen_tr, options.get().len());
                if dr.contains(point) {
                    return Some(id);
                }
            }
        }

        self.hit_test_recursive(root, point, Point::ZERO)
    }

    fn hit_test_recursive(&self, id: NodeId, point: Point, scroll_offset: Point) -> Option<NodeId> {
        let node = self.nodes.get(id)?;
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return None;
        }
        match &node.kind {
            WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                if !open.get() =>
            {
                return None;
            }
            WidgetKind::Drawer { open, .. } if !open.get() => {
                return None;
            }
            _ => {}
        }

        let base_rect = self
            .layout_engine
            .rect(id)
            .translate(scroll_offset.x, scroll_offset.y);
        let local_xf = self.node_effective_transform(id, base_rect.center());
        let local_point = if local_xf.is_identity() {
            point
        } else {
            let inv = local_xf.inverse()?;
            inv.transform_point(point)
        };
        let rect = base_rect.inflate(node.box_style.hit_slop, node.box_style.hit_slop);

        if node.box_style.clip && !rect.contains(local_point) {
            return None;
        }

        if let WidgetKind::SplitPane {
            axis,
            ratio,
            min_first,
            min_second,
            divider_size,
            ..
        } = &node.kind
        {
            let inner = base_rect.inset(node.layout.padding);
            let (_, _, div_rect) = Self::split_divider_geometry(
                inner,
                *axis,
                ratio.get_untracked(),
                *min_first,
                *min_second,
                *divider_size,
            );
            if base_rect.contains(local_point) && div_rect.inflate(4.0, 4.0).contains(local_point) {
                return Some(id);
            }
        }

        let mut child_offset = scroll_offset;
        if let WidgetKind::ScrollView { axis, offset, .. } = &node.kind {
            match axis {
                Axis::Vertical => child_offset.y -= *offset,
                Axis::Horizontal => child_offset.x -= *offset,
            }
        }

        let skip_children = matches!(
            &node.kind,
            WidgetKind::Accordion { expanded, .. } if !expanded.get_untracked()
        );
        if !skip_children {
            // Topmost Z-order children first
            for &child in node.children.iter().rev() {
                if let Some(hit) = self.hit_test_recursive(child, local_point, child_offset) {
                    return Some(hit);
                }
            }
        }

        let hit_self = match &node.kind {
            WidgetKind::SplitPane { .. } => {
                rect.contains(local_point)
                    && (node.on_tap.is_some()
                        || node.on_double_tap.is_some()
                        || node.on_long_press.is_some()
                        || node.on_hover.is_some()
                        || node.on_pinch_rotate.is_some())
            }
            WidgetKind::Accordion { .. } => {
                let header_r = Rect::new(
                    base_rect.x,
                    base_rect.y,
                    base_rect.width,
                    44.0_f32.min(base_rect.height),
                )
                .inflate(node.box_style.hit_slop, node.box_style.hit_slop);
                header_r.contains(local_point)
            }
            WidgetKind::ToastHost { items, .. } => {
                let (cards, _) = Self::toast_card_rects(base_rect, items.get().len());
                cards.iter().any(|c| c.contains(local_point))
            }
            _ => rect.contains(local_point) && self.is_interactive(node),
        };
        if hit_self { Some(id) } else { None }
    }

    fn is_interactive(&self, node: &WidgetNode) -> bool {
        node.on_tap.is_some()
            || node.on_double_tap.is_some()
            || node.on_long_press.is_some()
            || node.on_hover.is_some()
            || node.on_pinch_rotate.is_some()
            || matches!(
                node.kind,
                WidgetKind::Button { .. }
                    | WidgetKind::Canvas {
                        on_pointer: Some(_),
                        ..
                    }
                    | WidgetKind::ScrollView { .. }
                    | WidgetKind::VirtualList { .. }
                    | WidgetKind::TextInput { .. }
                    | WidgetKind::Checkbox { .. }
                    | WidgetKind::Switch { .. }
                    | WidgetKind::Slider { .. }
                    | WidgetKind::Tabs { .. }
                    | WidgetKind::SegmentedControl { .. }
                    | WidgetKind::BottomNavBar { .. }
                    | WidgetKind::FloatingActionButton { .. }
                    | WidgetKind::ListTile { .. }
                    | WidgetKind::Chip { .. }
                    | WidgetKind::AppBar { .. }
                    | WidgetKind::Drawer { .. }
                    | WidgetKind::Accordion { .. }
                    | WidgetKind::DataTable { .. }
                    | WidgetKind::Select { .. }
                    | WidgetKind::RadioGroup { .. }
                    | WidgetKind::PopupMenu { .. }
                    | WidgetKind::Tooltip { .. }
                    | WidgetKind::SplitPane { .. }
                    | WidgetKind::ToastHost { .. }
                    | WidgetKind::Overlay { .. }
                    | WidgetKind::Custom(_)
            )
    }

    /// Dispatch a [`UiEvent`] through the gesture arena, scroll physics, keyboard, and IME.
    /// Returns `true` if the UI state/scene should be re-rendered.
    pub fn handle_event(&mut self, event: &UiEvent) -> bool {
        match event {
            UiEvent::Pointer(pe) => self.handle_pointer(pe),
            UiEvent::Scroll(delta) => self.handle_scroll(delta.position, delta.x, delta.y),
            UiEvent::Key(ke) => self.handle_key(ke),
            UiEvent::Ime(ime) => self.handle_ime(ime),
            UiEvent::Pinch { factor, center } => self.dispatch_pinch_rotate(*center, *factor, 0.0),
            UiEvent::Resize {
                width,
                height,
                scale_factor,
            } => {
                self.metrics.logical_size = Size::new(*width, *height);
                self.metrics.scale_factor = *scale_factor;
                self.metrics_signal.set(self.metrics);
                true
            }
            UiEvent::FocusChanged(_) => false,
        }
    }

    fn dispatch_pinch_rotate(
        &mut self,
        center: Point,
        scale_delta: f32,
        angle_delta_rad: f32,
    ) -> bool {
        let mut cursor = self.hit_test(center).or(self.hovered).or(self.root);
        while let Some(nid) = cursor {
            let parent = self.nodes.get(nid).and_then(|n| n.parent);
            if let Some(cb) = self.nodes.get(nid).and_then(|n| n.on_pinch_rotate.clone()) {
                if let Ok(mut f) = cb.try_borrow_mut() {
                    f(center, scale_delta, angle_delta_rad);
                }
                return true;
            }
            cursor = parent;
        }
        false
    }

    fn handle_pointer(&mut self, pe: &PointerEvent) -> bool {
        self.last_pointer_pos = Some(pe.position);
        let mut changed = false;
        let hit = self.hit_test(pe.position);

        // Close open Select dropdowns when clicking outside them
        if matches!(pe.phase, PointerPhase::Down) {
            let open_selects: Vec<NodeId> = self
                .nodes
                .iter()
                .filter_map(|(id, n)| {
                    if matches!(n.kind, WidgetKind::Select { open: true, .. }) && hit != Some(id) {
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect();
            for sid in open_selects {
                if let Some(n) = self.nodes.get_mut(sid)
                    && let WidgetKind::Select { open, .. } = &mut n.kind
                {
                    *open = false;
                    n.dirty |= DirtyFlags::PAINT;
                    changed = true;
                }
            }
        }

        // Update hovered state, hover control animations, and fire on_hover callbacks
        if hit != self.hovered {
            if let Some(prev) = self.hovered {
                if let Some(n) = self.nodes.get_mut(prev) {
                    let mut unhover = false;
                    match &mut n.kind {
                        WidgetKind::Button { hovered, .. }
                        | WidgetKind::FloatingActionButton { hovered, .. }
                        | WidgetKind::ListTile { hovered, .. }
                        | WidgetKind::Chip { hovered, .. }
                            if *hovered =>
                        {
                            *hovered = false;
                            unhover = true;
                        }
                        _ => {}
                    }
                    if unhover {
                        n.dirty |= DirtyFlags::PAINT;
                        changed = true;
                    }
                }
                if let Some(ControlVisualAnim::Slider { hover_t, .. }) =
                    self.control_anims.get_mut(prev)
                {
                    hover_t.set_target(0.0);
                    if let Some(n) = self.nodes.get_mut(prev) {
                        n.dirty |= DirtyFlags::PAINT;
                    }
                    changed = true;
                }
                self.sync_control_anim_target(prev);
                if let Some(cb) = self.nodes.get(prev).and_then(|n| n.on_hover.clone())
                    && let Ok(mut f) = cb.try_borrow_mut()
                {
                    f(false);
                    changed = true;
                }
            }
            self.hovered = hit;
            if let Some(curr) = hit {
                let dis = self.nodes.get(curr).is_some_and(WidgetNode::is_disabled);
                if let Some(n) = self.nodes.get_mut(curr) {
                    let mut do_hover = false;
                    match &mut n.kind {
                        WidgetKind::Button { hovered, .. }
                        | WidgetKind::FloatingActionButton { hovered, .. }
                        | WidgetKind::ListTile { hovered, .. }
                        | WidgetKind::Chip { hovered, .. }
                            if !*hovered =>
                        {
                            *hovered = true;
                            do_hover = true;
                        }
                        _ => {}
                    }
                    if do_hover {
                        n.dirty |= DirtyFlags::PAINT;
                        changed = true;
                    }
                }
                if !dis
                    && let Some(ControlVisualAnim::Slider { hover_t, .. }) =
                        self.control_anims.get_mut(curr)
                {
                    hover_t.set_target(1.0);
                    if let Some(n) = self.nodes.get_mut(curr) {
                        n.dirty |= DirtyFlags::PAINT;
                    }
                    changed = true;
                }
                self.sync_control_anim_target(curr);
                if let Some(cb) = self.nodes.get(curr).and_then(|n| n.on_hover.clone())
                    && let Ok(mut f) = cb.try_borrow_mut()
                {
                    f(true);
                    changed = true;
                }
            }
            if self.inspector.enabled {
                self.inspector.hovered_node = hit.map(|h| {
                    let name = self.nodes.get(h).map(|n| n.debug_name).unwrap_or("Node");
                    let xr = self
                        .node_world_transform(h)
                        .transform_rect_bounds(self.node_visual_rect(h));
                    (name.to_string(), xr)
                });
                changed = true;
            } else if self
                .nodes
                .values()
                .any(|n| matches!(n.kind, WidgetKind::Tooltip { .. }))
            {
                changed = true;
            }
        }

        // Route raw pointer (mapped into local widget coords) to captured Canvas or CustomWidget
        let target = self.gesture_arena.active_capture(pe.id).or(hit);
        if let Some(t) = target {
            let rect = self.node_visual_rect(t);
            let mut local_pe = *pe;
            local_pe.position = self.screen_to_node_local_point(t, pe.position);
            let kind = self.nodes.get(t).map(|n| n.kind.clone());
            match kind {
                Some(WidgetKind::Canvas {
                    on_pointer: Some(cb),
                    ..
                }) => {
                    if let Ok(mut func) = cb.try_borrow_mut() {
                        func(rect, &local_pe);
                        if !matches!(local_pe.phase, PointerPhase::Move)
                            || local_pe.button.is_some()
                        {
                            changed = true;
                        }
                    }
                }
                Some(WidgetKind::Custom(cw)) => {
                    if let Ok(mut guard) = cw.try_borrow_mut()
                        && guard.handle_event(rect, &UiEvent::Pointer(local_pe))
                    {
                        changed = true;
                    }
                }
                _ => {}
            }
        }

        // Button / Slider / SplitPane sash press feedback
        if matches!(pe.phase, PointerPhase::Down)
            && let Some(h) = hit
            && !self.nodes.get(h).is_some_and(WidgetNode::is_disabled)
        {
            let h_rect = self.node_visual_rect(h);
            let h_local = self.screen_to_node_local_point(h, pe.position);
            if let Some(n) = self.nodes.get_mut(h) {
                match &mut n.kind {
                    WidgetKind::Button { pressed, .. }
                    | WidgetKind::FloatingActionButton { pressed, .. }
                    | WidgetKind::ListTile { pressed, .. } => {
                        *pressed = true;
                        n.dirty |= DirtyFlags::PAINT;
                        changed = true;
                    }
                    WidgetKind::SplitPane {
                        axis,
                        ratio,
                        min_first,
                        min_second,
                        divider_size,
                        dragging,
                        ..
                    } => {
                        let inner = h_rect.inset(n.layout.padding);
                        let (_, _, div_rect) = Self::split_divider_geometry(
                            inner,
                            *axis,
                            ratio.get_untracked(),
                            *min_first,
                            *min_second,
                            *divider_size,
                        );
                        if div_rect.inflate(4.0, 4.0).contains(h_local) {
                            *dragging = true;
                            n.dirty |= DirtyFlags::PAINT;
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ControlVisualAnim::Slider {
                dragging, drag_t, ..
            }) = self.control_anims.get_mut(h)
            {
                *dragging = true;
                drag_t.set_target(1.0);
                if let Some(n) = self.nodes.get_mut(h) {
                    n.dirty |= DirtyFlags::PAINT;
                }
                changed = true;
            }
            self.sync_control_anim_target(h);
        }
        if matches!(pe.phase, PointerPhase::Up | PointerPhase::Cancel) {
            let mut released = Vec::new();
            for (nid, node) in &mut self.nodes {
                match &mut node.kind {
                    WidgetKind::Button { pressed, .. }
                    | WidgetKind::FloatingActionButton { pressed, .. }
                    | WidgetKind::ListTile { pressed, .. }
                        if *pressed =>
                    {
                        *pressed = false;
                        node.dirty |= DirtyFlags::PAINT;
                        released.push(nid);
                        changed = true;
                    }
                    WidgetKind::SplitPane { dragging, .. } if *dragging => {
                        *dragging = false;
                        node.dirty |= DirtyFlags::PAINT;
                        changed = true;
                    }
                    _ => {}
                }
            }
            for nid in released {
                self.sync_control_anim_target(nid);
            }
            let slider_ids: Vec<NodeId> = self.control_anims.keys().collect();
            for sid in slider_ids {
                if let Some(ControlVisualAnim::Slider {
                    dragging, drag_t, ..
                }) = self.control_anims.get_mut(sid)
                    && *dragging
                {
                    *dragging = false;
                    drag_t.set_target(0.0);
                    if let Some(n) = self.nodes.get_mut(sid) {
                        n.dirty |= DirtyFlags::PAINT;
                    }
                    changed = true;
                }
            }
        }

        let gestures = self.gesture_arena.process_pointer(pe, hit);
        for g in gestures {
            match g {
                RecognizedGesture::Tap { node, position } => {
                    if !self.nodes.get(node).is_some_and(WidgetNode::is_disabled) {
                        self.focused = Some(node);
                        self.focus_visible = false;
                        self.activate_tap(node, position, true);
                        changed = true;
                    }
                }
                RecognizedGesture::DoubleTap { node, .. } => {
                    if !self.nodes.get(node).is_some_and(WidgetNode::is_disabled)
                        && let Some(cb) = self.nodes.get(node).and_then(|n| n.on_double_tap.clone())
                        && let Ok(mut f) = cb.try_borrow_mut()
                    {
                        f();
                        changed = true;
                    }
                }
                RecognizedGesture::LongPress { node, .. } => {
                    if !self.nodes.get(node).is_some_and(WidgetNode::is_disabled)
                        && let Some(cb) = self.nodes.get(node).and_then(|n| n.on_long_press.clone())
                        && let Ok(mut f) = cb.try_borrow_mut()
                    {
                        f();
                        changed = true;
                    }
                }
                RecognizedGesture::DragStart { node, position } => {
                    self.handle_drag(node, position, Point::ZERO);
                    changed = true;
                }
                RecognizedGesture::DragUpdate {
                    node,
                    position,
                    delta,
                } => {
                    self.handle_drag(node, position, delta);
                    changed = true;
                }
                RecognizedGesture::DragEnd { node, velocity } => {
                    self.handle_drag_end(node, velocity);
                    changed = true;
                }
                RecognizedGesture::PinchRotate {
                    center,
                    scale_delta,
                    angle_delta_rad,
                } => {
                    if self.dispatch_pinch_rotate(center, scale_delta, angle_delta_rad) {
                        changed = true;
                    }
                }
            }
        }

        changed
    }

    fn activate_tap(&mut self, node_id: NodeId, pos: Point, is_screen_pos: bool) {
        let rect = self.node_visual_rect(node_id);

        if self.nodes.get(node_id).is_some_and(WidgetNode::is_disabled) {
            return;
        }

        let (local_pos, screen_pos) = if is_screen_pos {
            (self.screen_to_node_local_point(node_id, pos), pos)
        } else {
            (pos, self.node_world_transform(node_id).transform_point(pos))
        };

        // Generic node on_tap handler
        let extra_tap = self.nodes.get(node_id).and_then(|n| n.on_tap.clone());
        if let Some(cb) = extra_tap
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f();
        }

        let mut action: Option<TapHandler> = None;
        let mut bool_cb: Option<(BoolCallback, bool)> = None;
        let mut float_cb: Option<(FloatCallback, f32)> = None;
        let mut idx_cb: Option<(IndexCallback, usize)> = None;
        let mut sort_cb: Option<(IndexCallback, usize)> = None;
        let mut dismiss_cb: Option<(ToastDismissCallback, u64)> = None;
        let mut reset_caret = false;
        let mut relayout_node = false;

        // Compute popup/dropdown geometry upfront to satisfy borrow checker
        let popup_rect_opt = match self.nodes.get(node_id).map(|n| &n.kind) {
            Some(WidgetKind::PopupMenu { items, anchor, .. }) => {
                Some(self.popup_menu_rect(*anchor, items.get().len(), rect))
            }
            _ => None,
        };
        let select_drop_rect_opt = match self.nodes.get(node_id).map(|n| &n.kind) {
            Some(WidgetKind::Select { options, .. }) => {
                let screen_tr = self
                    .node_world_transform(node_id)
                    .transform_rect_bounds(rect);
                Some(self.select_dropdown_rect(screen_tr, options.get().len()))
            }
            _ => None,
        };

        if let Some(node) = self.nodes.get_mut(node_id) {
            match &mut node.kind {
                WidgetKind::Button {
                    disabled, on_tap, ..
                }
                | WidgetKind::FloatingActionButton {
                    disabled, on_tap, ..
                } => {
                    if !disabled.get() {
                        action = on_tap.clone();
                    }
                }
                WidgetKind::Checkbox {
                    checked, on_change, ..
                }
                | WidgetKind::Switch {
                    checked, on_change, ..
                } => {
                    let next = !checked.get_untracked();
                    checked.set(next);
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    if let Some(cb) = on_change.clone() {
                        bool_cb = Some((cb, next));
                    }
                }
                WidgetKind::Slider {
                    value,
                    min,
                    max,
                    on_change,
                    ..
                } => {
                    let track_x = rect.x + 8.0;
                    let track_w = (rect.width - 16.0).max(1.0);
                    let t = ((local_pos.x - track_x) / track_w).clamp(0.0, 1.0);
                    let next = *min + (*max - *min) * t;
                    value.set(next);
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    if let Some(cb) = on_change.clone() {
                        float_cb = Some((cb, next));
                    }
                }
                WidgetKind::TextInput {
                    editor,
                    style,
                    multiline,
                    ..
                } => {
                    let pad_top = if *multiline { 10.0 } else { 8.0 };
                    let inner_w = (rect.width - 24.0).max(40.0);
                    let wrap = if *multiline { Some(inner_w) } else { None };
                    let inner_x = local_pos.x - (rect.x + 12.0);
                    let inner_y = local_pos.y - (rect.y + pad_top);
                    let layout =
                        self.text_engine
                            .build_layout_cached(&editor.display_text(), style, wrap);
                    let byte_idx = self.text_engine.hit_test_point(&layout, inner_x, inner_y);
                    editor.set_cursor_at(byte_idx, false);
                    reset_caret = true;
                }
                WidgetKind::Tabs {
                    tabs,
                    active,
                    on_change,
                } => {
                    let count = tabs.get().len();
                    if count > 0 && rect.width > 0.0 {
                        let idx =
                            (((local_pos.x - rect.x) / rect.width) * count as f32).floor() as isize;
                        let next = (idx.clamp(0, (count - 1) as isize)) as usize;
                        active.set(next);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_change.clone() {
                            idx_cb = Some((cb, next));
                        }
                    }
                }
                WidgetKind::SegmentedControl {
                    items,
                    selected,
                    on_change,
                }
                | WidgetKind::BottomNavBar {
                    items,
                    selected,
                    on_change,
                    ..
                } => {
                    let count = items.get().len();
                    if count > 0 && rect.width > 0.0 {
                        let idx =
                            (((local_pos.x - rect.x) / rect.width) * count as f32).floor() as isize;
                        let next = idx.clamp(0, (count - 1) as isize) as usize;
                        selected.set(next);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_change.clone() {
                            idx_cb = Some((cb, next));
                        }
                    }
                }
                WidgetKind::Accordion {
                    expanded,
                    on_change,
                    ..
                } => {
                    if !is_screen_pos || local_pos.y <= rect.y + 44.0 {
                        let next = !expanded.get_untracked();
                        expanded.set(next);
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y;
                        relayout_node = true;
                        if let Some(cb) = on_change.clone() {
                            bool_cb = Some((cb, next));
                        }
                    }
                }
                WidgetKind::DataTable {
                    columns,
                    rows,
                    selected,
                    sort_column,
                    sort_ascending,
                    scroll_offset,
                    row_height,
                    on_select,
                    on_sort,
                    ..
                } => {
                    let cols = columns.get();
                    let row_list = rows.get();
                    let header_h = 38.0f32.min(rect.height);
                    if is_screen_pos && local_pos.y <= rect.y + header_h && !cols.is_empty() {
                        let col_w = (rect.width / cols.len() as f32).max(1.0);
                        let ci = (((local_pos.x - rect.x) / col_w).floor() as isize)
                            .clamp(0, (cols.len() - 1) as isize)
                            as usize;
                        if *sort_column == Some(ci) {
                            *sort_ascending = !*sort_ascending;
                        } else {
                            *sort_column = Some(ci);
                            *sort_ascending = true;
                        }
                        node.dirty |= DirtyFlags::PAINT;
                        if let Some(cb) = on_sort.clone() {
                            sort_cb = Some((cb, ci));
                        }
                    } else if !row_list.is_empty() && *row_height > 0.0 {
                        let sorted = Self::data_table_sorted_indices(
                            &row_list,
                            *sort_column,
                            *sort_ascending,
                        );
                        let rel_y = if is_screen_pos {
                            (local_pos.y - (rect.y + header_h) + *scroll_offset).max(0.0)
                        } else {
                            0.0
                        };
                        let vis_idx = ((rel_y / *row_height).floor() as isize)
                            .clamp(0, (sorted.len() - 1) as isize)
                            as usize;
                        let orig_idx = sorted[vis_idx];
                        selected.set(orig_idx);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_select.clone() {
                            idx_cb = Some((cb, orig_idx));
                        }
                    }
                }
                WidgetKind::Select {
                    options,
                    selected,
                    open,
                    dropdown_scroll,
                    on_change,
                    ..
                } => {
                    let opts = options.get();
                    if *open {
                        if let Some(dr) = select_drop_rect_opt
                            && dr.contains(screen_pos)
                            && !opts.is_empty()
                        {
                            let row = ((screen_pos.y - dr.y - 4.0 + *dropdown_scroll) / 34.0)
                                .floor() as isize;
                            let next = row.clamp(0, (opts.len() - 1) as isize) as usize;
                            selected.set(next);
                            *open = false;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        } else {
                            *open = false;
                        }
                    } else {
                        *open = true;
                        if let Some(dr) = select_drop_rect_opt {
                            let total_h = opts.len() as f32 * 34.0 + 8.0;
                            let max_scroll = (total_h - dr.height).max(0.0);
                            if max_scroll <= 0.0 {
                                *dropdown_scroll = 0.0;
                            } else {
                                let sel_idx =
                                    selected.get_untracked().min(opts.len().saturating_sub(1));
                                let item_top = sel_idx as f32 * 34.0;
                                let item_bottom = item_top + 34.0;
                                let vis_h = (dr.height - 8.0).max(34.0);
                                let mut next = (*dropdown_scroll).clamp(0.0, max_scroll);
                                if item_top < next {
                                    next = item_top;
                                } else if item_bottom > next + vis_h {
                                    next = (item_bottom - vis_h).clamp(0.0, max_scroll);
                                }
                                *dropdown_scroll = next.clamp(0.0, max_scroll);
                            }
                        }
                    }
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                }
                WidgetKind::RadioGroup {
                    options,
                    selected,
                    axis,
                    on_change,
                } => {
                    let count = options.get().len();
                    if count > 0 {
                        let next = match axis {
                            Axis::Vertical => (((local_pos.y - rect.y) / 30.0).floor() as isize)
                                .clamp(0, (count - 1) as isize)
                                as usize,
                            Axis::Horizontal => (((local_pos.x - rect.x)
                                / (rect.width.max(1.0) / count as f32))
                                .floor() as isize)
                                .clamp(0, (count - 1) as isize)
                                as usize,
                        };
                        selected.set(next);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        if let Some(cb) = on_change.clone() {
                            idx_cb = Some((cb, next));
                        }
                    }
                }
                WidgetKind::PopupMenu {
                    items,
                    on_select,
                    on_close,
                    ..
                } => {
                    let count = items.get().len();
                    if let Some(mr) = popup_rect_opt
                        && mr.contains(local_pos)
                        && count > 0
                    {
                        let idx = (((local_pos.y - mr.y - 4.0) / 36.0).floor() as isize)
                            .clamp(0, (count - 1) as isize)
                            as usize;
                        if let Some(cb) = on_select.clone() {
                            idx_cb = Some((cb, idx));
                        }
                    }
                    action = on_close.clone();
                }
                WidgetKind::ToastHost {
                    items,
                    on_dismiss,
                    manager,
                } => {
                    let list = items.get();
                    let (_cards, closes) = Self::toast_card_rects(rect, list.len());
                    for (idx, close_r) in closes.iter().enumerate() {
                        if close_r.inflate(8.0, 8.0).contains(local_pos) {
                            let dismissed_id = list[idx].id;
                            if let Some(mgr) = manager {
                                mgr.dismiss(dismissed_id);
                            }
                            if let Some(cb) = on_dismiss.clone() {
                                dismiss_cb = Some((cb, dismissed_id));
                            }
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            break;
                        }
                    }
                }
                WidgetKind::ListTile { on_tap, .. } => {
                    action = on_tap.clone();
                }
                WidgetKind::Chip {
                    selected, on_tap, ..
                } => {
                    let next = !selected.get_untracked();
                    selected.set(next);
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    action = on_tap.clone();
                }
                WidgetKind::AppBar { on_leading_tap, .. } => {
                    action = on_leading_tap.clone();
                }
                WidgetKind::Drawer {
                    open,
                    width,
                    on_close,
                } => {
                    if open.get_untracked() {
                        let drawer_w = (*width).min(rect.width * 0.85).max(240.0);
                        if local_pos.x > rect.x + drawer_w {
                            open.set(false);
                            node.dirty |= DirtyFlags::PAINT;
                            action = on_close.clone();
                        }
                    }
                }
                WidgetKind::Overlay { on_close, .. } => {
                    action = on_close.clone();
                }
                _ => {}
            }
        }

        self.sync_control_anim_target(node_id);

        if relayout_node {
            self.layout_engine.mark_dirty(node_id);
        }

        if reset_caret {
            self.reset_caret_blink(craftui_core::monotonic_ms());
        }
        if let Some((cb, v)) = bool_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(v);
        }
        if let Some((cb, v)) = float_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(v);
        }
        if let Some((cb, i)) = idx_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(i);
        }
        if let Some((cb, col)) = sort_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(col);
        }
        if let Some((cb, id)) = dismiss_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(id);
        }
        if let Some(cb) = action
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f();
        }
    }

    fn handle_drag(&mut self, node_id: NodeId, pos: Point, delta: Point) {
        let mut cursor = Some(node_id);
        while let Some(nid) = cursor {
            let parent = self.nodes.get(nid).and_then(|n| n.parent);
            let r = self.node_visual_rect(nid);
            let layout_r = self.layout_engine.rect(nid);
            let local_pos = self.screen_to_node_local_point(nid, pos);
            let world_xf = self.node_world_transform(nid);
            let local_delta = if world_xf.is_identity() {
                delta
            } else if let Some(inv) = world_xf.inverse() {
                let [a, b, c, d, _, _] = inv.matrix;
                Point::new(a * delta.x + c * delta.y, b * delta.x + d * delta.y)
            } else {
                delta
            };
            let mut slider_cb: Option<(FloatCallback, f32, f32)> = None;
            let mut matched = false;
            if let Some(node) = self.nodes.get_mut(nid) {
                match &mut node.kind {
                    WidgetKind::Slider {
                        value,
                        min,
                        max,
                        on_change,
                        ..
                    } => {
                        let track_x = r.x + 8.0;
                        let track_w = (r.width - 16.0).max(1.0);
                        let t = ((local_pos.x - track_x) / track_w).clamp(0.0, 1.0);
                        let next = *min + (*max - *min) * t;
                        value.set(next);
                        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                        slider_cb = Some((
                            on_change.clone().unwrap_or_else(|| {
                                std::rc::Rc::new(std::cell::RefCell::new(|_| {}))
                            }),
                            t,
                            next,
                        ));
                        matched = true;
                    }
                    WidgetKind::SplitPane {
                        axis,
                        ratio,
                        min_first,
                        min_second,
                        divider_size,
                        dragging,
                        on_change,
                    } => {
                        let inner = r.inset(node.layout.padding);
                        let div = (*divider_size).max(4.0);
                        let (_, _, div_rect) = Self::split_divider_geometry(
                            inner,
                            *axis,
                            ratio.get_untracked(),
                            *min_first,
                            *min_second,
                            div,
                        );
                        if *dragging || div_rect.inflate(8.0, 8.0).contains(local_pos) {
                            let total = match axis {
                                Axis::Horizontal => inner.width,
                                Axis::Vertical => inner.height,
                            };
                            let usable = (total - div).max(1.0);
                            let min_f = (*min_first).max(0.0).min(usable);
                            let max_f = (usable - (*min_second).max(0.0)).max(min_f);
                            let coord = match axis {
                                Axis::Horizontal => local_pos.x - inner.x,
                                Axis::Vertical => local_pos.y - inner.y,
                            };
                            let first_px = (coord - div * 0.5).clamp(min_f, max_f);
                            let next_ratio = (first_px / usable).clamp(0.0, 1.0);
                            *dragging = true;
                            ratio.set(next_ratio);
                            node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                            slider_cb = Some((
                                on_change.clone().unwrap_or_else(|| {
                                    std::rc::Rc::new(std::cell::RefCell::new(|_| {}))
                                }),
                                next_ratio,
                                next_ratio,
                            ));
                            matched = true;
                        }
                    }
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => layout_r.height,
                            Axis::Horizontal => layout_r.width,
                        };
                        let max_off = (*content_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -local_delta.y,
                            Axis::Horizontal => -local_delta.x,
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::PAINT;
                        return;
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => layout_r.height,
                            Axis::Horizontal => layout_r.width,
                        };
                        let total_rows = item_count.get().div_ceil((*columns).max(1));
                        let max_off = (total_rows as f32 * *item_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -local_delta.y,
                            Axis::Horizontal => -local_delta.x,
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                        return;
                    }
                    _ => {}
                }
            }
            if matched {
                if let Some((cb, frac, next)) = slider_cb {
                    if let Some(ControlVisualAnim::Slider {
                        frac_t,
                        dragging,
                        drag_t,
                        ..
                    }) = self.control_anims.get_mut(nid)
                    {
                        *dragging = true;
                        drag_t.set_target(1.0);
                        frac_t.snap(frac);
                    }
                    if let Ok(mut f) = cb.try_borrow_mut() {
                        f(next);
                    }
                }
                return;
            }
            cursor = parent;
        }
    }

    fn handle_drag_end(&mut self, node_id: NodeId, velocity: (f32, f32)) {
        if let Some(ControlVisualAnim::Slider {
            dragging, drag_t, ..
        }) = self.control_anims.get_mut(node_id)
        {
            *dragging = false;
            drag_t.set_target(0.0);
            if let Some(n) = self.nodes.get_mut(node_id) {
                n.dirty |= DirtyFlags::PAINT;
            }
        }
        if let Some(n) = self.nodes.get_mut(node_id)
            && let WidgetKind::SplitPane { dragging, .. } = &mut n.kind
            && *dragging
        {
            *dragging = false;
            n.dirty |= DirtyFlags::PAINT;
        }
        let mut cursor = Some(node_id);
        while let Some(nid) = cursor {
            let parent = self.nodes.get(nid).and_then(|n| n.parent);
            if let Some(node) = self.nodes.get_mut(nid) {
                match &mut node.kind {
                    WidgetKind::ScrollView {
                        axis,
                        velocity: vel,
                        ..
                    }
                    | WidgetKind::VirtualList {
                        axis,
                        velocity: vel,
                        ..
                    } => {
                        *vel = match axis {
                            Axis::Vertical => velocity.1,
                            Axis::Horizontal => velocity.0,
                        };
                        return;
                    }
                    _ => {}
                }
            }
            cursor = parent;
        }
    }

    fn handle_scroll(&mut self, pos: Point, dx: f32, dy: f32) -> bool {
        if let Some(root) = self.root {
            let mut order = Vec::new();
            self.collect_preorder(root, &mut order);
            for id in order.into_iter().rev() {
                let drop_info = self.nodes.get(id).and_then(|node| {
                    if !node.layout.hidden
                        && let WidgetKind::Select {
                            open: true,
                            options,
                            ..
                        } = &node.kind
                    {
                        let count = options.get().len();
                        let tr = self.node_visual_rect(id);
                        let screen_tr = self.node_world_transform(id).transform_rect_bounds(tr);
                        let dr = self.select_dropdown_rect(screen_tr, count);
                        if dr.contains(pos) {
                            return Some((dr, count));
                        }
                    }
                    None
                });
                if let Some((dr, count)) = drop_info
                    && let Some(node) = self.nodes.get_mut(id)
                    && let WidgetKind::Select {
                        dropdown_scroll, ..
                    } = &mut node.kind
                {
                    let total_h = count as f32 * 34.0 + 8.0;
                    let max_scroll = (total_h - dr.height).max(0.0);
                    *dropdown_scroll = (*dropdown_scroll - dy).clamp(0.0, max_scroll);
                    node.dirty |= DirtyFlags::PAINT;
                    return true;
                }
            }
        }

        let Some(mut cursor) = self.hit_test(pos).or(self.root) else {
            return false;
        };
        loop {
            let rect = self.layout_engine.rect(cursor);
            let parent = self.nodes.get(cursor).and_then(|n| n.parent);
            if let Some(node) = self.nodes.get_mut(cursor) {
                match &mut node.kind {
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => rect.height,
                            Axis::Horizontal => rect.width,
                        };
                        let max_off = (*content_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -dy,
                            Axis::Horizontal => {
                                if dx.abs() > 0.01 {
                                    -dx
                                } else {
                                    -dy
                                }
                            }
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::PAINT;
                        return true;
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => rect.height,
                            Axis::Horizontal => rect.width,
                        };
                        let total_rows = item_count.get().div_ceil((*columns).max(1));
                        let max_off = (total_rows as f32 * *item_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -dy,
                            Axis::Horizontal => {
                                if dx.abs() > 0.01 {
                                    -dx
                                } else {
                                    -dy
                                }
                            }
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                        return true;
                    }
                    WidgetKind::DataTable {
                        rows,
                        scroll_offset,
                        row_height,
                        ..
                    } => {
                        let header_h = 38.0f32.min(rect.height);
                        let body_h = (rect.height - header_h).max(0.0);
                        let total_h = rows.get().len() as f32 * *row_height;
                        let max_off = (total_h - body_h).max(0.0);
                        *scroll_offset = (*scroll_offset - dy).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::PAINT;
                        return true;
                    }
                    _ => {}
                }
            }
            let Some(p) = parent else {
                break;
            };
            cursor = p;
        }
        false
    }

    /// Dismiss the topmost open `Select` dropdown, `PopupMenu`, or `Overlay` (`Modal`/`BottomSheet`).
    fn dismiss_top_overlay(&mut self) -> bool {
        let Some(root) = self.root else {
            return false;
        };
        let mut order = Vec::new();
        self.collect_preorder(root, &mut order);

        for id in order.into_iter().rev() {
            if let Some(node) = self.nodes.get_mut(id) {
                match &mut node.kind {
                    WidgetKind::Select { open: true, .. } => {
                        if let WidgetKind::Select { open, .. } = &mut node.kind {
                            *open = false;
                            node.dirty |= DirtyFlags::PAINT;
                        }
                        return true;
                    }
                    WidgetKind::PopupMenu { open, on_close, .. } if open.get() => {
                        let cb = on_close.clone();
                        if let Some(f) = cb
                            && let Ok(mut func) = f.try_borrow_mut()
                        {
                            func();
                            return true;
                        }
                    }
                    WidgetKind::Overlay { open, on_close, .. } if open.get() => {
                        let cb = on_close.clone();
                        if let Some(f) = cb
                            && let Ok(mut func) = f.try_borrow_mut()
                        {
                            func();
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }

    fn active_focus_root(&self) -> Option<NodeId> {
        let root = self.root?;
        let mut order = Vec::new();
        self.collect_preorder(root, &mut order);
        for id in order.into_iter().rev() {
            if let Some(n) = self.nodes.get(id)
                && !n.layout.hidden
            {
                match &n.kind {
                    WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                        if open.get() =>
                    {
                        return Some(id);
                    }
                    _ => {}
                }
            }
        }
        Some(root)
    }

    fn collect_preorder(&self, id: NodeId, out: &mut Vec<NodeId>) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return;
        }
        match &node.kind {
            WidgetKind::Overlay { open, .. } | WidgetKind::PopupMenu { open, .. }
                if !open.get() =>
            {
                return;
            }
            WidgetKind::Drawer { open, .. } if !open.get() => {
                return;
            }
            WidgetKind::Accordion { expanded, .. } if !expanded.get() => {
                out.push(id);
                return;
            }
            _ => {}
        }
        out.push(id);
        for &c in &node.children {
            self.collect_preorder(c, out);
        }
    }

    fn advance_focus(&mut self, reverse: bool) -> bool {
        let Some(scope_root) = self.active_focus_root() else {
            return false;
        };
        let mut nodes = Vec::new();
        self.collect_preorder(scope_root, &mut nodes);
        let focusables: Vec<NodeId> = nodes
            .into_iter()
            .filter(|&id| {
                self.nodes
                    .get(id)
                    .is_some_and(|n| n.a11y.focusable && !n.is_disabled())
            })
            .collect();
        if focusables.is_empty() {
            return false;
        }
        let next = if let Some(cur) = self.focused
            && let Some(pos) = focusables.iter().position(|&id| id == cur)
        {
            if reverse {
                let prev = if pos == 0 {
                    focusables.len() - 1
                } else {
                    pos - 1
                };
                focusables[prev]
            } else {
                focusables[(pos + 1) % focusables.len()]
            }
        } else if reverse {
            *focusables.last().expect("non-empty focusables")
        } else {
            focusables[0]
        };

        if let Some(old) = self.focused
            && let Some(on) = self.nodes.get_mut(old)
        {
            on.dirty |= DirtyFlags::PAINT;
        }
        self.focused = Some(next);
        self.focus_visible = true;
        if let Some(nn) = self.nodes.get_mut(next) {
            nn.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        }
        self.reset_caret_blink(craftui_core::monotonic_ms());
        true
    }

    fn handle_key(&mut self, ke: &KeyEvent) -> bool {
        if !ke.pressed {
            return false;
        }

        // Ctrl+Shift+I / Cmd+Shift+I toggles Live Inspector Overlay (PRD FR-9.2)
        if ke.modifiers.command() && ke.modifiers.shift && ke.code.matches_ascii_letter('I') {
            self.inspector.enabled = !self.inspector.enabled;
            return true;
        }

        // Escape dismisses topmost active dropdown/popup/modal
        if matches!(ke.code, KeyCode::Escape) && self.dismiss_top_overlay() {
            return true;
        }

        // Tab / Shift+Tab traverses focusable widgets cleanly (trapping inside open modal)
        if matches!(ke.code, KeyCode::Tab) {
            return self.advance_focus(ke.modifiers.shift);
        }

        let Some(focused) = self.focused else {
            return false;
        };
        let focused_rect = self.layout_engine.rect(focused);

        // Forward keys directly to focused CustomWidget if applicable
        if let Some(WidgetKind::Custom(cw)) = self.nodes.get(focused).map(|n| n.kind.clone()) {
            let rect = self.node_visual_rect(focused);
            if let Ok(mut guard) = cw.try_borrow_mut()
                && guard.handle_event(rect, &UiEvent::Key(ke.clone()))
            {
                return true;
            }
        }

        let mut text_changed = None;
        let mut submitted = None;
        let mut activated = false;
        let mut reset_caret = false;
        let mut idx_cb: Option<(IndexCallback, usize)> = None;
        let mut float_cb: Option<(FloatCallback, f32)> = None;

        // Read clipboard outside node borrow
        let paste_clip = if ke.modifiers.command() && ke.code.matches_ascii_letter('V') {
            self.services
                .read_clipboard()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    if self.clipboard_text.is_empty() {
                        None
                    } else {
                        Some(self.clipboard_text.clone())
                    }
                })
        } else {
            None
        };
        let mut copy_clip: Option<String> = None;

        if let Some(node) = self.nodes.get_mut(focused) {
            if node.is_disabled() {
                return false;
            }
            match &mut node.kind {
                WidgetKind::TextInput {
                    editor,
                    multiline,
                    bound_signal,
                    ..
                } => {
                    let prev = editor.text.clone();
                    match &ke.code {
                        KeyCode::Backspace if ke.modifiers.word_jump() => {
                            editor.delete_word_backward();
                        }
                        KeyCode::Backspace => editor.backspace(),
                        KeyCode::Delete if ke.modifiers.word_jump() => {
                            editor.delete_word_forward();
                        }
                        KeyCode::Delete => editor.delete_forward(),
                        KeyCode::ArrowLeft if ke.modifiers.word_jump() => {
                            editor.move_word_left(ke.modifiers.shift);
                        }
                        KeyCode::ArrowLeft => editor.move_left(ke.modifiers.shift),
                        KeyCode::ArrowRight if ke.modifiers.word_jump() => {
                            editor.move_word_right(ke.modifiers.shift);
                        }
                        KeyCode::ArrowRight => editor.move_right(ke.modifiers.shift),
                        KeyCode::ArrowUp if *multiline => editor.move_up(ke.modifiers.shift),
                        KeyCode::ArrowDown if *multiline => editor.move_down(ke.modifiers.shift),
                        KeyCode::Home => editor.move_home(ke.modifiers.shift),
                        KeyCode::End => editor.move_end(ke.modifiers.shift),
                        code if ke.modifiers.command() && code.matches_ascii_letter('A') => {
                            editor.select_all();
                        }
                        code if ke.modifiers.command() && code.matches_ascii_letter('C') => {
                            let sel = editor.selected_text().to_string();
                            copy_clip = Some(sel);
                        }
                        code if ke.modifiers.command() && code.matches_ascii_letter('X') => {
                            let sel = editor.selected_text().to_string();
                            copy_clip = Some(sel);
                            editor.delete_selection();
                        }
                        code if ke.modifiers.command() && code.matches_ascii_letter('V') => {
                            if let Some(ref clip) = paste_clip {
                                editor.insert_str(clip);
                            }
                        }
                        code if ke.modifiers.command()
                            && !ke.modifiers.shift
                            && code.matches_ascii_letter('Z') =>
                        {
                            let _ = editor.undo();
                        }
                        code if (ke.modifiers.command()
                            && ke.modifiers.shift
                            && code.matches_ascii_letter('Z'))
                            || (ke.modifiers.ctrl
                                && !ke.modifiers.shift
                                && code.matches_ascii_letter('Y')) =>
                        {
                            let _ = editor.redo();
                        }
                        KeyCode::Enter => {
                            if *multiline && !ke.modifiers.command() {
                                editor.insert_str("\n");
                            } else {
                                submitted = Some(editor.text.clone());
                            }
                        }
                        _ => {
                            if let Some(ref txt) = ke.text
                                && !ke.modifiers.command()
                                && !ke.modifiers.ctrl
                                && !ke.modifiers.alt
                                && !txt.chars().all(|c| c.is_control())
                            {
                                editor.insert_str(txt);
                            }
                        }
                    }
                    if editor.text != prev {
                        if let Some(sig) = bound_signal {
                            sig.set(editor.text.clone());
                        }
                        node.a11y.value = Some(editor.text.clone());
                        text_changed = Some(editor.text.clone());
                    }
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    reset_caret = true;
                }
                WidgetKind::Button { .. }
                | WidgetKind::Checkbox { .. }
                | WidgetKind::Switch { .. }
                | WidgetKind::Accordion { .. }
                | WidgetKind::ListTile { .. }
                | WidgetKind::Chip { .. } => {
                    if matches!(ke.code, KeyCode::Enter | KeyCode::Space) {
                        activated = true;
                    }
                }
                WidgetKind::Tabs {
                    tabs,
                    active,
                    on_change,
                } => {
                    let count = tabs.get().len();
                    if count > 0 {
                        let cur = active.get_untracked().min(count - 1);
                        let next = match ke.code {
                            KeyCode::ArrowRight => (cur + 1) % count,
                            KeyCode::ArrowLeft => {
                                if cur == 0 {
                                    count - 1
                                } else {
                                    cur - 1
                                }
                            }
                            _ => cur,
                        };
                        if next != cur {
                            active.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                    }
                }
                WidgetKind::SegmentedControl {
                    items,
                    selected,
                    on_change,
                }
                | WidgetKind::BottomNavBar {
                    items,
                    selected,
                    on_change,
                    ..
                } => {
                    let count = items.get().len();
                    if count > 0 {
                        let cur = selected.get_untracked().min(count - 1);
                        let next = match ke.code {
                            KeyCode::ArrowRight => (cur + 1) % count,
                            KeyCode::ArrowLeft => {
                                if cur == 0 {
                                    count - 1
                                } else {
                                    cur - 1
                                }
                            }
                            KeyCode::Home => 0,
                            KeyCode::End => count - 1,
                            _ => cur,
                        };
                        if next != cur {
                            selected.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                    }
                }
                WidgetKind::DataTable {
                    rows,
                    selected,
                    sort_column,
                    sort_ascending,
                    scroll_offset,
                    row_height,
                    on_select,
                    ..
                } => {
                    let row_list = rows.get();
                    if !row_list.is_empty() {
                        let sorted = Self::data_table_sorted_indices(
                            &row_list,
                            *sort_column,
                            *sort_ascending,
                        );
                        let cur_orig = selected.get_untracked();
                        let cur_vis = sorted.iter().position(|&r| r == cur_orig).unwrap_or(0);
                        let next_vis = match ke.code {
                            KeyCode::ArrowDown => (cur_vis + 1).min(sorted.len() - 1),
                            KeyCode::ArrowUp => cur_vis.saturating_sub(1),
                            KeyCode::Home => 0,
                            KeyCode::End => sorted.len() - 1,
                            _ => cur_vis,
                        };
                        if next_vis != cur_vis || sorted[next_vis] != cur_orig {
                            let orig_idx = sorted[next_vis];
                            selected.set(orig_idx);
                            let body_h = (focused_rect.height - 38.0).max(0.0);
                            if *row_height > 0.0 && body_h > 0.0 {
                                let total_h = sorted.len() as f32 * *row_height;
                                let max_scroll = (total_h - body_h).max(0.0);
                                let item_top = next_vis as f32 * *row_height;
                                let item_bottom = item_top + *row_height;
                                if item_top < *scroll_offset {
                                    *scroll_offset = item_top.clamp(0.0, max_scroll);
                                } else if item_bottom > *scroll_offset + body_h {
                                    *scroll_offset = (item_bottom - body_h).clamp(0.0, max_scroll);
                                }
                            }
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_select.clone() {
                                idx_cb = Some((cb, orig_idx));
                            }
                        }
                    }
                }
                WidgetKind::Select {
                    options,
                    selected,
                    open,
                    dropdown_scroll,
                    on_change,
                    ..
                } => {
                    let count = options.get().len();
                    let drop_h = (count.max(1) as f32 * 34.0 + 8.0).min(240.0);
                    let total_h = count as f32 * 34.0 + 8.0;
                    let max_scroll = (total_h - drop_h).max(0.0);
                    let ensure_sel_visible = |sel: usize, scroll: &mut f32| {
                        if max_scroll <= 0.0 {
                            *scroll = 0.0;
                            return;
                        }
                        let item_top = sel as f32 * 34.0;
                        let item_bottom = item_top + 34.0;
                        let vis_h = (drop_h - 8.0).max(34.0);
                        let mut next = (*scroll).clamp(0.0, max_scroll);
                        if item_top < next {
                            next = item_top;
                        } else if item_bottom > next + vis_h {
                            next = (item_bottom - vis_h).clamp(0.0, max_scroll);
                        }
                        *scroll = next.clamp(0.0, max_scroll);
                    };
                    match ke.code {
                        KeyCode::Enter | KeyCode::Space => {
                            *open = !*open;
                            if *open && count > 0 {
                                let cur = selected.get_untracked().min(count - 1);
                                ensure_sel_visible(cur, dropdown_scroll);
                            }
                            node.dirty |= DirtyFlags::PAINT;
                        }
                        KeyCode::ArrowDown if count > 0 => {
                            let next = (selected.get_untracked() + 1).min(count - 1);
                            selected.set(next);
                            if *open {
                                ensure_sel_visible(next, dropdown_scroll);
                            }
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                        KeyCode::ArrowUp if count > 0 => {
                            let next = selected.get_untracked().saturating_sub(1);
                            selected.set(next);
                            if *open {
                                ensure_sel_visible(next, dropdown_scroll);
                            }
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                        _ => {}
                    }
                }
                WidgetKind::RadioGroup {
                    options,
                    selected,
                    on_change,
                    ..
                } => {
                    let count = options.get().len();
                    if count > 0 {
                        let cur = selected.get_untracked().min(count - 1);
                        let next = match ke.code {
                            KeyCode::ArrowDown | KeyCode::ArrowRight => (cur + 1) % count,
                            KeyCode::ArrowUp | KeyCode::ArrowLeft => {
                                if cur == 0 {
                                    count - 1
                                } else {
                                    cur - 1
                                }
                            }
                            _ => cur,
                        };
                        if next != cur {
                            selected.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                idx_cb = Some((cb, next));
                            }
                        }
                    }
                }
                WidgetKind::Slider {
                    value,
                    min,
                    max,
                    on_change,
                    ..
                } => {
                    let step = (*max - *min) * 0.05;
                    match ke.code {
                        KeyCode::ArrowRight | KeyCode::ArrowUp => {
                            let next = (value.get_untracked() + step).clamp(*min, *max);
                            value.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                float_cb = Some((cb, next));
                            }
                        }
                        KeyCode::ArrowLeft | KeyCode::ArrowDown => {
                            let next = (value.get_untracked() - step).clamp(*min, *max);
                            value.set(next);
                            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                            if let Some(cb) = on_change.clone() {
                                float_cb = Some((cb, next));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        self.sync_control_anim_target(focused);

        if reset_caret {
            self.reset_caret_blink(craftui_core::monotonic_ms());
        }

        if let Some(clip) = copy_clip {
            self.clipboard_text = clip.clone();
            self.services.write_clipboard(&clip);
        }

        if activated {
            let center = self.node_visual_rect(focused).center();
            self.activate_tap(focused, center, false);
        }

        if let Some((cb, i)) = idx_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(i);
        }
        if let Some((cb, v)) = float_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(v);
        }

        if let Some(txt) = text_changed {
            let cb = self.nodes.get(focused).and_then(|n| match &n.kind {
                WidgetKind::TextInput { on_change, .. } => on_change.clone(),
                _ => None,
            });
            if let Some(func) = cb
                && let Ok(mut f) = func.try_borrow_mut()
            {
                f(txt);
            }
        }

        if let Some(txt) = submitted {
            let cb = self.nodes.get(focused).and_then(|n| match &n.kind {
                WidgetKind::TextInput { on_submit, .. } => on_submit.clone(),
                _ => None,
            });
            if let Some(func) = cb
                && let Ok(mut f) = func.try_borrow_mut()
            {
                f(txt);
            }
        }

        true
    }

    fn handle_ime(&mut self, ime: &craftui_core::ImeEvent) -> bool {
        let Some(focused) = self.focused else {
            return false;
        };
        let Some(node) = self.nodes.get_mut(focused) else {
            return false;
        };
        let mut changed_cb: Option<(crate::widgets::TextCallback, String)> = None;
        let WidgetKind::TextInput {
            editor,
            bound_signal,
            on_change,
            ..
        } = &mut node.kind
        else {
            return false;
        };
        match ime {
            craftui_core::ImeEvent::Preedit(s, cur) => {
                editor.preedit = if s.is_empty() { None } else { Some(s.clone()) };
                editor.preedit_cursor = *cur;
            }
            craftui_core::ImeEvent::Commit(s) => {
                editor.preedit = None;
                editor.preedit_cursor = None;
                editor.insert_str(s);
                node.a11y.value = Some(editor.text.clone());
                if let Some(sig) = bound_signal {
                    sig.set(editor.text.clone());
                }
                if let Some(cb) = on_change.clone() {
                    changed_cb = Some((cb, editor.text.clone()));
                }
            }
            craftui_core::ImeEvent::Disabled => {
                editor.preedit = None;
                editor.preedit_cursor = None;
            }
            craftui_core::ImeEvent::Enabled => {}
        }
        node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        if let Some((cb, txt)) = changed_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(txt);
        }
        self.reset_caret_blink(craftui_core::monotonic_ms());
        true
    }

    /// Reset incremental AccessKit cache so the next [`build_a11y_update`](Self::build_a11y_update)
    /// emits a full initial tree snapshot.
    pub fn invalidate_a11y(&mut self) {
        self.a11y_bridge.invalidate();
    }

    /// Collect incremental AccessKit tree update for screen readers.
    /// Returns `None` when no accessibility node or focus state changed since the last update.
    pub fn build_a11y_update(&mut self) -> Option<craftui_a11y::accesskit::TreeUpdate> {
        let root = self.root?;
        let mut snaps = Vec::new();
        self.collect_a11y_recursive(root, &mut snaps, &mut HashSet::new());
        self.a11y_bridge
            .build_incremental_update(root, self.focused, &snaps)
    }

    fn collect_a11y_recursive(
        &self,
        id: NodeId,
        out: &mut Vec<A11ySnapshotNode>,
        seen: &mut HashSet<NodeId>,
    ) {
        if !seen.insert(id) {
            return;
        }
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden || node.taffy_hidden_by_parent {
            return;
        }
        let is_collapsed = matches!(
            &node.kind,
            WidgetKind::Accordion { expanded, .. } if !expanded.get()
        ) || matches!(
            &node.kind,
            WidgetKind::Drawer { open, .. } if !open.get()
        );
        let child_ids: Vec<NodeId> = if is_collapsed {
            Vec::new()
        } else {
            node.children
                .iter()
                .copied()
                .filter(|&cid| {
                    self.nodes
                        .get(cid)
                        .is_some_and(|cn| !cn.layout.hidden && !cn.taffy_hidden_by_parent)
                })
                .collect()
        };
        let bounds = self.layout_engine.rect(id);
        out.push(A11ySnapshotNode {
            id,
            bounds,
            meta: node.a11y.clone(),
            children: child_ids.clone(),
        });
        for c in child_ids {
            self.collect_a11y_recursive(c, out, seen);
        }
    }

    pub fn handle_a11y_action(&mut self, req: &craftui_a11y::accesskit::ActionRequest) -> bool {
        let Some(action) = self.a11y_bridge.translate_action(req) else {
            return false;
        };
        match action {
            A11yAction::Click(id) => {
                let c = self.node_visual_rect(id).center();
                self.activate_tap(id, c, false);
                true
            }
            A11yAction::Focus(id) => {
                self.focused = Some(id);
                self.reset_caret_blink(craftui_core::monotonic_ms());
                true
            }
            A11yAction::SetValue(id, v) => {
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::TextInput {
                        editor,
                        bound_signal,
                        ..
                    } = &mut n.kind
                {
                    editor.sync_external(&v);
                    n.a11y.value = Some(v.clone());
                    n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    if let Some(sig) = bound_signal {
                        sig.set(v);
                    }
                    self.reset_caret_blink(craftui_core::monotonic_ms());
                    return true;
                }
                false
            }
            A11yAction::SetNumericValue(id, v) => {
                let ok = if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    value.set((v as f32).clamp(*min, *max));
                    n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    true
                } else {
                    false
                };
                if ok {
                    self.sync_control_anim_target(id);
                }
                ok
            }
            A11yAction::Increment(id) => {
                let ok = if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    let step = (*max - *min) * 0.05;
                    value.update(|v| *v = (*v + step).clamp(*min, *max));
                    n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    true
                } else {
                    false
                };
                if ok {
                    self.sync_control_anim_target(id);
                }
                ok
            }
            A11yAction::Decrement(id) => {
                let ok = if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    let step = (*max - *min) * 0.05;
                    value.update(|v| *v = (*v - step).clamp(*min, *max));
                    n.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                    true
                } else {
                    false
                };
                if ok {
                    self.sync_control_anim_target(id);
                }
                ok
            }
            A11yAction::ScrollIntoView(_) => false,
        }
    }

    fn node_path(&self, target: NodeId) -> Option<Vec<usize>> {
        let root = self.root?;
        let mut rev = Vec::new();
        let mut cur = target;
        while cur != root {
            let parent = self.nodes.get(cur)?.parent?;
            let pnode = self.nodes.get(parent)?;
            let idx = pnode.children.iter().position(|&c| c == cur)?;
            rev.push(idx);
            cur = parent;
        }
        rev.reverse();
        Some(rev)
    }

    fn node_at_path(&self, path: &[usize]) -> Option<NodeId> {
        let mut cur = self.root?;
        for &idx in path {
            cur = *self.nodes.get(cur)?.children.get(idx)?;
        }
        Some(cur)
    }

    /// Capture transient focus, scroll offsets, text cursor positions, and open selects for hot-reload preservation.
    pub fn capture_ui_state(&self) -> TreeUiSnapshot {
        let mut snap = TreeUiSnapshot {
            focused_path: self.focused.and_then(|id| self.node_path(id)),
            ..Default::default()
        };
        let Some(root) = self.root else {
            return snap;
        };
        let mut stack = vec![(root, Vec::<usize>::new())];
        while let Some((id, path)) = stack.pop() {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            match &node.kind {
                WidgetKind::ScrollView { offset, .. }
                | WidgetKind::VirtualList { offset, .. }
                | WidgetKind::DataTable {
                    scroll_offset: offset,
                    ..
                } => {
                    if *offset > 0.0 {
                        snap.scroll_offsets.push((path.clone(), *offset));
                    }
                }
                WidgetKind::TextInput { editor, .. } => {
                    snap.editor_states
                        .push((path.clone(), editor.cursor, editor.anchor));
                }
                WidgetKind::Select { open: true, .. } => {
                    snap.select_open_paths.push(path.clone());
                }
                _ => {}
            }
            for (idx, &child) in node.children.iter().enumerate() {
                let mut child_path = path.clone();
                child_path.push(idx);
                stack.push((child, child_path));
            }
        }
        snap
    }

    /// Restore transient UI state captured before a hot-reload subtree replacement.
    pub fn restore_ui_state(&mut self, snap: &TreeUiSnapshot) {
        if let Some(ref fpath) = snap.focused_path
            && let Some(fid) = self.node_at_path(fpath)
            && self
                .nodes
                .get(fid)
                .is_some_and(|n| n.a11y.focusable && !n.is_disabled())
        {
            self.focused = Some(fid);
            if self.focused_is_text_input() {
                self.reset_caret_blink(craftui_core::monotonic_ms());
            }
        }
        for (path, off) in &snap.scroll_offsets {
            if let Some(id) = self.node_at_path(path)
                && let Some(n) = self.nodes.get_mut(id)
            {
                match &mut n.kind {
                    WidgetKind::ScrollView { offset, .. }
                    | WidgetKind::VirtualList { offset, .. }
                    | WidgetKind::DataTable {
                        scroll_offset: offset,
                        ..
                    } => {
                        *offset = (*off).max(0.0);
                        n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                    }
                    _ => {}
                }
            }
        }
        let clamp_utf8 = |s: &str, pos: usize| -> usize {
            let mut p = pos.min(s.len());
            while p > 0 && !s.is_char_boundary(p) {
                p -= 1;
            }
            p
        };
        for (path, cursor, anchor) in &snap.editor_states {
            if let Some(id) = self.node_at_path(path)
                && let Some(n) = self.nodes.get_mut(id)
                && let WidgetKind::TextInput { editor, .. } = &mut n.kind
            {
                editor.cursor = clamp_utf8(&editor.text, *cursor);
                editor.anchor = clamp_utf8(&editor.text, *anchor);
                n.dirty |= DirtyFlags::PAINT;
            }
        }
        for path in &snap.select_open_paths {
            if let Some(id) = self.node_at_path(path)
                && let Some(n) = self.nodes.get_mut(id)
                && let WidgetKind::Select { open, .. } = &mut n.kind
            {
                *open = true;
                n.dirty |= DirtyFlags::PAINT;
            }
        }
    }
}
