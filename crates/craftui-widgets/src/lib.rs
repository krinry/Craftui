//! Retained widget tree, standard widget primitives, gestures, animations, router, and inspector.

pub mod anim;
pub mod gesture;
pub mod inspector;
pub mod router;
pub mod toast;
pub mod tree;
pub mod widgets;

pub use anim::{
    AnimatableProp, AnimatedColor, AnimatedFloat, Easing, Spring, TransitionCurve, Tween,
};
pub use gesture::{GestureArena, RecognizedGesture, ScrollPhysics, VelocityTracker};
pub use inspector::InspectorState;
pub use router::{Router, RouterTransition};
pub use toast::{ToastItem, ToastManager, ToastSeverity};
pub use tree::{TreeUiSnapshot, VirtualItemFactory, WidgetNode, WidgetTree};
pub use widgets::{
    BoolCallback, BoxStyle, CanvasPaintFn, CanvasPointerFn, CardVariant, CustomWidget,
    FloatCallback, HoverCallback, IndexCallback, PinchRotateCallback, ReactiveValue, TapHandler,
    TextCallback, ToastDismissCallback, WidgetKind,
};

/// Composable declarative view mountable onto a [`WidgetTree`].
pub trait View {
    fn build(self, cx: craftui_reactive::Scope, tree: &mut WidgetTree) -> craftui_core::NodeId;
}

impl View for craftui_core::NodeId {
    fn build(self, _cx: craftui_reactive::Scope, _tree: &mut WidgetTree) -> craftui_core::NodeId {
        self
    }
}

/// Closure-backed [`View`] returned by `cui!` macro invocations and `#[component]` functions.
pub struct ViewFn<F>(pub F);

impl<F> ViewFn<F> {
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F> View for ViewFn<F>
where
    F: FnOnce(craftui_reactive::Scope, &mut WidgetTree) -> craftui_core::NodeId,
{
    fn build(self, cx: craftui_reactive::Scope, tree: &mut WidgetTree) -> craftui_core::NodeId {
        (self.0)(cx, tree)
    }
}

impl WidgetTree {
    pub fn mount(&mut self, cx: craftui_reactive::Scope, view: impl View) -> craftui_core::NodeId {
        let id = view.build(cx, self);
        self.set_root(id);
        id
    }
}

#[cfg(test)]
mod tests;
