use crate::toast::{ToastItem, ToastManager};
use craftui_a11y::A11yMetadata;
use craftui_core::{
    Axis, BlendMode, Border, BoxShadow, Brush, Color, CornerRadii, CursorIcon, Fit, PlatformViewId,
    Point, Rect, Size, TextureId, ThemeTokens, Transform2D, UiEvent,
};
use craftui_reactive::{Computed, ReadSignal, Signal};
use craftui_render::PaintScene;
use craftui_text::{RichText, TextEditorState, TextEngine, TextStyle};
use std::cell::RefCell;
use std::rc::Rc;

/// A property value that can be either static or lazily read from reactive signals.
#[derive(Clone)]
pub enum ReactiveValue<T: Clone + 'static> {
    Static(T),
    Dynamic(Rc<dyn Fn() -> T>),
}

impl<T: Clone + 'static> ReactiveValue<T> {
    pub fn get(&self) -> T {
        match self {
            Self::Static(v) => v.clone(),
            Self::Dynamic(f) => f(),
        }
    }

    pub fn is_dynamic(&self) -> bool {
        matches!(self, Self::Dynamic(_))
    }
}

impl<T: Clone + Default + 'static> Default for ReactiveValue<T> {
    fn default() -> Self {
        Self::Static(T::default())
    }
}

impl<T: Clone + 'static> From<T> for ReactiveValue<T> {
    fn from(value: T) -> Self {
        Self::Static(value)
    }
}

impl<T: Clone + 'static> From<Signal<T>> for ReactiveValue<T> {
    fn from(sig: Signal<T>) -> Self {
        Self::Dynamic(Rc::new(move || sig.get()))
    }
}

impl<T: Clone + 'static> From<ReadSignal<T>> for ReactiveValue<T> {
    fn from(sig: ReadSignal<T>) -> Self {
        Self::Dynamic(Rc::new(move || sig.get()))
    }
}

impl<T: Clone + 'static> From<Computed<T>> for ReactiveValue<T> {
    fn from(comp: Computed<T>) -> Self {
        Self::Dynamic(Rc::new(move || comp.get()))
    }
}

impl From<&str> for ReactiveValue<String> {
    fn from(value: &str) -> Self {
        Self::Static(value.to_string())
    }
}

impl<F> From<F> for ReactiveValue<String>
where
    F: Fn() -> String + 'static,
{
    fn from(f: F) -> Self {
        Self::Dynamic(Rc::new(f))
    }
}

/// Visual box decoration shared by containers, buttons, cards, and inputs.
#[derive(Clone)]
pub struct BoxStyle {
    pub background: Option<ReactiveValue<Brush>>,
    pub border: Option<Border>,
    pub border_color: Option<ReactiveValue<Color>>,
    pub shadow: Option<BoxShadow>,
    pub shadows: Vec<BoxShadow>,
    pub backdrop_blur: f32,
    pub backdrop_tint: Color,
    pub transform: Transform2D,
    pub blend_mode: BlendMode,
    pub cursor: Option<CursorIcon>,
    pub radius: CornerRadii,
    pub opacity: f32,
    pub clip: bool,
    pub hit_slop: f32,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl BoxStyle {
    pub fn new() -> Self {
        Self {
            background: None,
            border: None,
            border_color: None,
            shadow: None,
            shadows: Vec::new(),
            backdrop_blur: 0.0,
            backdrop_tint: Color::TRANSPARENT,
            transform: Transform2D::IDENTITY,
            blend_mode: BlendMode::Normal,
            cursor: None,
            radius: CornerRadii::ZERO,
            opacity: 1.0,
            clip: false,
            hit_slop: 0.0,
        }
    }

    pub fn with_shadow(mut self, shadow: BoxShadow) -> Self {
        self.shadows.push(shadow);
        self
    }

    pub fn with_backdrop_blur(mut self, blur_radius: f32, tint: Color) -> Self {
        self.backdrop_blur = blur_radius.max(0.0);
        self.backdrop_tint = tint;
        self
    }

    pub fn with_transform(mut self, transform: Transform2D) -> Self {
        self.transform = transform;
        self
    }

    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    pub fn with_cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = Some(cursor);
        self
    }
}

pub type TapHandler = Rc<RefCell<dyn FnMut()>>;
pub type HoverCallback = Rc<RefCell<dyn FnMut(bool)>>;
pub type BoolCallback = Rc<RefCell<dyn FnMut(bool)>>;
pub type FloatCallback = Rc<RefCell<dyn FnMut(f32)>>;
pub type IndexCallback = Rc<RefCell<dyn FnMut(usize)>>;
pub type TextCallback = Rc<RefCell<dyn FnMut(String)>>;
pub type PinchRotateCallback = Rc<RefCell<dyn FnMut(Point, f32, f32)>>;
pub type CanvasPaintFn = Rc<dyn Fn(Rect, &mut PaintScene)>;
pub type CanvasPointerFn = Rc<RefCell<dyn FnMut(Rect, &craftui_core::PointerEvent)>>;
pub type ToastDismissCallback = Rc<RefCell<dyn FnMut(u64)>>;

/// Public extension trait enabling downstream applications to implement custom
/// layout, painting, input handling, and accessibility widgets without editing `craftui-widgets`.
pub trait CustomWidget {
    fn debug_name(&self) -> &'static str {
        "CustomWidget"
    }

    fn measure(
        &mut self,
        constraints: craftui_core::Constraints,
        _text_engine: &mut TextEngine,
        _theme: &ThemeTokens,
    ) -> Size {
        let w = if constraints.max_width.is_finite() {
            constraints.max_width.min(120.0)
        } else {
            120.0
        };
        let h = if constraints.max_height.is_finite() {
            constraints.max_height.min(40.0)
        } else {
            40.0
        };
        Size::new(w.max(constraints.min_width), h.max(constraints.min_height))
    }

    fn paint(
        &mut self,
        rect: Rect,
        scene: &mut PaintScene,
        text_engine: &mut TextEngine,
        theme: &ThemeTokens,
        focused: bool,
    );

    fn handle_event(&mut self, _rect: Rect, _event: &UiEvent) -> bool {
        false
    }

    /// Whether this custom widget can hold keyboard focus via pointer clicks or Tab traversal.
    fn focusable(&self) -> bool {
        false
    }

    /// Whether this custom widget intercepts `Tab` key presses instead of advancing focus.
    fn wants_tab(&self) -> bool {
        false
    }

    /// Whether this custom widget receives key-release (`pressed: false`) events while focused.
    fn wants_key_up(&self) -> bool {
        false
    }

    /// Whether this visible custom widget should automatically receive focus on keyboard input
    /// when no other visible widget currently holds focus.
    fn autofocus(&self) -> bool {
        false
    }

    fn a11y(&self) -> A11yMetadata {
        A11yMetadata::default()
    }
}

/// Concrete widget behavior stored in a retained [`WidgetNode`](crate::WidgetNode).
#[derive(Clone)]
pub enum WidgetKind {
    Container,
    Spacer,
    SafeArea,
    Divider {
        axis: Axis,
        thickness: f32,
        color: Option<ReactiveValue<Color>>,
    },
    Text {
        content: ReactiveValue<String>,
        style: ReactiveValue<TextStyle>,
    },
    /// A paragraph shaped as a single unit with independently styled spans.
    RichText {
        content: ReactiveValue<RichText>,
        on_link: Option<TextCallback>,
    },
    Icon {
        svg: ReactiveValue<String>,
        size: f32,
        color: Option<ReactiveValue<Color>>,
    },
    Button {
        label: ReactiveValue<String>,
        disabled: ReactiveValue<bool>,
        foreground: Option<ReactiveValue<Color>>,
        on_tap: Option<TapHandler>,
        hovered: bool,
        pressed: bool,
    },
    Image {
        texture: ReactiveValue<Option<TextureId>>,
        fit: Fit,
        radius: CornerRadii,
        opacity: f32,
    },
    Canvas {
        on_paint: CanvasPaintFn,
        on_pointer: Option<CanvasPointerFn>,
    },
    ScrollView {
        axis: Axis,
        offset: f32,
        velocity: f32,
        content_extent: f32,
    },
    VirtualList {
        axis: Axis,
        columns: usize,
        item_extent: f32,
        item_count: ReactiveValue<usize>,
        offset: f32,
        velocity: f32,
    },
    TextInput {
        editor: TextEditorState,
        placeholder: String,
        style: TextStyle,
        multiline: bool,
        bound_signal: Option<Signal<String>>,
        on_change: Option<TextCallback>,
        on_submit: Option<TextCallback>,
    },
    Checkbox {
        checked: Signal<bool>,
        label: String,
        on_change: Option<BoolCallback>,
    },
    Switch {
        checked: Signal<bool>,
        label: String,
        on_change: Option<BoolCallback>,
    },
    Slider {
        value: Signal<f32>,
        min: f32,
        max: f32,
        label: String,
        on_change: Option<FloatCallback>,
    },
    Tabs {
        tabs: ReactiveValue<Vec<String>>,
        active: Signal<usize>,
        on_change: Option<IndexCallback>,
    },
    Select {
        options: ReactiveValue<Vec<String>>,
        selected: Signal<usize>,
        open: bool,
        dropdown_scroll: f32,
        placeholder: String,
        on_change: Option<IndexCallback>,
    },
    RadioGroup {
        options: ReactiveValue<Vec<String>>,
        selected: Signal<usize>,
        axis: Axis,
        on_change: Option<IndexCallback>,
    },
    PopupMenu {
        open: ReactiveValue<bool>,
        items: ReactiveValue<Vec<String>>,
        anchor: Option<Point>,
        on_select: Option<IndexCallback>,
        on_close: Option<TapHandler>,
    },
    Tooltip {
        text: ReactiveValue<String>,
    },
    ProgressIndicator {
        progress: ReactiveValue<Option<f32>>,
        phase: f32,
    },
    Spinner {
        size: f32,
        thickness: f32,
        color: Option<ReactiveValue<Color>>,
        phase: f32,
    },
    Badge {
        label: ReactiveValue<String>,
        foreground: Option<ReactiveValue<Color>>,
        background: Option<ReactiveValue<Color>>,
        icon: Option<ReactiveValue<String>>,
    },
    SplitPane {
        axis: Axis,
        ratio: Signal<f32>,
        min_first: f32,
        min_second: f32,
        divider_size: f32,
        dragging: bool,
        on_change: Option<FloatCallback>,
    },
    ToastHost {
        items: ReactiveValue<Vec<ToastItem>>,
        on_dismiss: Option<ToastDismissCallback>,
        manager: Option<ToastManager>,
    },
    SegmentedControl {
        items: ReactiveValue<Vec<String>>,
        selected: Signal<usize>,
        on_change: Option<IndexCallback>,
    },
    BottomNavBar {
        items: ReactiveValue<Vec<String>>,
        icons: ReactiveValue<Vec<String>>,
        selected: Signal<usize>,
        on_change: Option<IndexCallback>,
    },
    FloatingActionButton {
        icon: ReactiveValue<String>,
        label: Option<ReactiveValue<String>>,
        disabled: ReactiveValue<bool>,
        foreground: Option<ReactiveValue<Color>>,
        background: Option<ReactiveValue<Color>>,
        on_tap: Option<TapHandler>,
        hovered: bool,
        pressed: bool,
    },
    Accordion {
        title: ReactiveValue<String>,
        expanded: Signal<bool>,
        on_change: Option<BoolCallback>,
    },
    DataTable {
        columns: ReactiveValue<Vec<String>>,
        rows: ReactiveValue<Vec<Vec<String>>>,
        selected: Signal<usize>,
        sort_column: Option<usize>,
        sort_ascending: bool,
        scroll_offset: f32,
        striped: bool,
        row_height: f32,
        on_select: Option<IndexCallback>,
        on_sort: Option<IndexCallback>,
    },
    Overlay {
        open: ReactiveValue<bool>,
        on_close: Option<TapHandler>,
        is_bottom_sheet: bool,
    },
    PlatformView {
        id: PlatformViewId,
    },
    HtmlView {
        src: ReactiveValue<String>,
        html: ReactiveValue<String>,
        router: Option<Rc<RefCell<craftui_core::IpcRouter>>>,
    },
    /// Material 3 Card panel container matching Flutter's `Card` (`Elevated`, `Filled`, `Outlined`).
    Card {
        variant: CardVariant,
    },
    /// Standard Material 3 item row matching Flutter's `ListTile`.
    ListTile {
        leading: Option<ReactiveValue<String>>,
        title: ReactiveValue<String>,
        subtitle: Option<ReactiveValue<String>>,
        trailing: Option<ReactiveValue<String>>,
        on_tap: Option<TapHandler>,
        hovered: bool,
        pressed: bool,
        selected: ReactiveValue<bool>,
    },
    /// Material 3 selectable or action chip matching Flutter's `Chip` / `FilterChip`.
    Chip {
        label: ReactiveValue<String>,
        icon: Option<ReactiveValue<String>>,
        selected: Signal<bool>,
        on_tap: Option<TapHandler>,
        hovered: bool,
    },
    /// Material 3 Top App Bar header matching Flutter's `AppBar`.
    AppBar {
        title: ReactiveValue<String>,
        leading_icon: Option<ReactiveValue<String>>,
        elevation: f32,
        on_leading_tap: Option<TapHandler>,
    },
    /// Slide-out navigation drawer panel matching Flutter's `Drawer`.
    Drawer {
        open: Signal<bool>,
        width: f32,
        on_close: Option<TapHandler>,
    },
    Custom(Rc<RefCell<dyn CustomWidget>>),
}

/// Material 3 Card visual style variants matching Flutter's `Card.elevated`, `Card.filled`, `Card.outlined`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardVariant {
    #[default]
    Elevated,
    Filled,
    Outlined,
}
