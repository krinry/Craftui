use std::cell::{Cell, RefCell};
use std::rc::Rc;

use craftui::prelude::*;

use crate::model::{ElementKind, HistoryManager, Tool, ViewTransform, WhiteboardElement};
use crate::sketch::{
    sketch_arrow_path, sketch_diamond_path, sketch_ellipse_path, sketch_freehand_path,
    sketch_line_path, sketch_rect_path,
};

#[derive(Debug, Clone)]
enum InteractionState {
    Idle,
    Panning { last_screen: Point },
    Drawing { draft: WhiteboardElement },
    DraggingElement { id: u64, last_world: Point },
    Erasing,
}

/// Shared reactive controller for the whiteboard canvas.
#[derive(Clone)]
pub struct WhiteboardController {
    pub elements: Signal<Vec<WhiteboardElement>>,
    pub tool: Signal<Tool>,
    pub stroke_color: Signal<Color>,
    pub stroke_width: Signal<f32>,
    pub view: Signal<ViewTransform>,
    pub selected_id: Signal<Option<u64>>,
    pub tool_locked: Signal<bool>,
    pub is_dark: Signal<bool>,
    pub history: Rc<RefCell<HistoryManager>>,
    next_id: Rc<Cell<u64>>,
    interaction: Rc<RefCell<InteractionState>>,
}

impl WhiteboardController {
    pub fn new(cx: Scope) -> Self {
        let elements = cx.signal(Self::sample_elements());
        let tool = cx.signal(Tool::Select);
        let stroke_color = cx.signal(Color::rgb(30, 30, 30));
        let stroke_width = cx.signal(2.5);
        let view = cx.signal(ViewTransform::default());
        let selected_id = cx.signal(None);
        let tool_locked = cx.signal(false);
        let is_dark = cx.signal(false);

        let mut history_mgr = HistoryManager::default();
        history_mgr.push(elements.get());

        Self {
            elements,
            tool,
            stroke_color,
            stroke_width,
            view,
            selected_id,
            tool_locked,
            is_dark,
            history: Rc::new(RefCell::new(history_mgr)),
            next_id: Rc::new(Cell::new(100)),
            interaction: Rc::new(RefCell::new(InteractionState::Idle)),
        }
    }

    /// Starter sample matching user screenshot (single hand-drawn arrow).
    fn sample_elements() -> Vec<WhiteboardElement> {
        vec![
            // The prominent arrow pointing right from the user screenshot!
            WhiteboardElement::new(
                1,
                Point::new(560.0, 240.0),
                ElementKind::Arrow {
                    end: Point::new(595.0, 246.0),
                },
                Color::rgb(30, 30, 30),
                None,
                2.5,
            ),
        ]
    }

    pub fn undo(&self) {
        let current = self.elements.get();
        if let Some(prev) = self.history.borrow_mut().undo(current) {
            self.elements.set(prev);
            self.selected_id.set(None);
        }
    }

    pub fn redo(&self) {
        let current = self.elements.get();
        if let Some(next) = self.history.borrow_mut().redo(current) {
            self.elements.set(next);
            self.selected_id.set(None);
        }
    }

    pub fn clear(&self) {
        let current = self.elements.get();
        self.history.borrow_mut().push(current);
        self.elements.set(Vec::new());
        self.selected_id.set(None);
    }

    pub fn zoom_in(&self, center: Point) {
        let mut v = self.view.get();
        v.zoom_at(1.2, center);
        self.view.set(v);
    }

    pub fn zoom_out(&self, center: Point) {
        let mut v = self.view.get();
        v.zoom_at(0.833, center);
        self.view.set(v);
    }

    pub fn zoom_reset(&self) {
        let mut v = self.view.get();
        v.zoom = 1.0;
        self.view.set(v);
    }

    pub fn select_tool(&self, t: Tool) {
        self.tool.set(t);
        if t != Tool::Select {
            self.selected_id.set(None);
        }
    }

    pub fn can_undo(&self) -> bool {
        self.history.borrow().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.borrow().can_redo()
    }

    pub fn delete_selected(&self) {
        if let Some(sel_id) = self.selected_id.get() {
            let current = self.elements.get();
            self.history.borrow_mut().push(current.clone());
            let filtered: Vec<_> = current.into_iter().filter(|e| e.id != sel_id).collect();
            self.elements.set(filtered);
            self.selected_id.set(None);
        }
    }
}

/// Spawns the interactive Whiteboard Canvas node in the widget tree.
pub fn spawn_whiteboard_canvas(
    tree: &mut WidgetTree,
    scope: Scope,
    layout: LayoutStyle,
    ctrl: WhiteboardController,
) -> NodeId {
    let ctrl_paint = ctrl.clone();
    let on_paint = move |rect: Rect, scene: &mut PaintScene| {
        paint_whiteboard(rect, scene, &ctrl_paint);
    };

    let ctrl_ptr = ctrl.clone();
    let on_pointer: CanvasPointerFn =
        Rc::new(RefCell::new(move |rect: Rect, event: &PointerEvent| {
            handle_pointer(rect, event, &ctrl_ptr);
        }));

    let node = tree.canvas(scope, layout, on_paint, Some(on_pointer));

    // Support pinch-to-zoom gestures (Android, iOS touchscreens, trackpads)
    let ctrl_pinch = ctrl.clone();
    tree.set_on_pinch_rotate(node, move |focal_pt, scale_delta, _rotation| {
        if scale_delta != 1.0 && scale_delta > 0.0 {
            let mut v = ctrl_pinch.view.get();
            v.zoom_at(scale_delta, focal_pt);
            ctrl_pinch.view.set(v);
        }
    });

    node
}

/// Attach whiteboard paint and pointer callbacks to a mounted Canvas node (from .cui DSL).
pub fn attach_whiteboard_canvas(tree: &mut WidgetTree, node: NodeId, ctrl: WhiteboardController) {
    let ctrl_paint = ctrl.clone();
    let on_paint: CanvasPaintFn = Rc::new(move |rect: Rect, scene: &mut PaintScene| {
        paint_whiteboard(rect, scene, &ctrl_paint);
    });

    let ctrl_ptr = ctrl.clone();
    let on_pointer: CanvasPointerFn =
        Rc::new(RefCell::new(move |rect: Rect, event: &PointerEvent| {
            handle_pointer(rect, event, &ctrl_ptr);
        }));

    tree.set_canvas_callbacks(node, Some(on_paint), Some(on_pointer));

    let ctrl_pinch = ctrl;
    tree.set_on_pinch_rotate(node, move |focal_pt, scale_delta, _rotation| {
        if scale_delta != 1.0 && scale_delta > 0.0 {
            let mut v = ctrl_pinch.view.get();
            v.zoom_at(scale_delta, focal_pt);
            ctrl_pinch.view.set(v);
        }
    });
}

pub fn paint_whiteboard(bounds: Rect, scene: &mut PaintScene, ctrl: &WhiteboardController) {
    let is_dark = ctrl.is_dark.get();
    let bg_color = if is_dark {
        Color::rgb(18, 18, 18)
    } else {
        Color::rgb(255, 255, 255)
    };
    scene.fill_rect(bounds, bg_color, CornerRadii::ZERO);

    let view = ctrl.view.get();

    // 1. Draw subtle infinite dot grid (batched in single fill_path call for high FPS)
    paint_dot_grid(bounds, scene, view, is_dark);

    // 2. Draw committed elements
    let elements = ctrl.elements.get();
    let selected_id = ctrl.selected_id.get();

    for elem in &elements {
        paint_element(scene, elem, view, is_dark);
    }

    // 3. Draw active drawing preview draft (if currently dragging out a new shape)
    let interaction = ctrl.interaction.borrow().clone();
    if let InteractionState::Drawing { draft } = interaction {
        paint_element(scene, &draft, view, is_dark);
    }

    // 4. Draw selection bounding box and corner handles
    if let Some(sel_id) = selected_id
        && let Some(sel_elem) = elements.iter().find(|e| e.id == sel_id)
    {
        paint_selection_halo(scene, sel_elem, view);
    }
}

fn paint_dot_grid(bounds: Rect, scene: &mut PaintScene, view: ViewTransform, is_dark: bool) {
    let grid_step = 36.0;
    let scaled_step = grid_step * view.zoom;

    if scaled_step < 16.0 {
        return; // Don't crowd screen when zoomed out
    }

    let start_x = ((view.pan.x % scaled_step) + scaled_step) % scaled_step;
    let start_y = ((view.pan.y % scaled_step) + scaled_step) % scaled_step;

    let dot_color = if is_dark {
        Color::rgba(65, 72, 85, (160.0 * view.zoom.min(1.0)) as u8)
    } else {
        Color::rgba(215, 220, 228, (160.0 * view.zoom.min(1.0)) as u8)
    };
    let dot_size = 1.5;

    let mut x = start_x;
    while x < bounds.width {
        let mut y = start_y;
        while y < bounds.height {
            scene.fill_rect(
                Rect::new(x, y, dot_size, dot_size),
                dot_color,
                CornerRadii::all(0.75),
            );
            y += scaled_step;
        }
        x += scaled_step;
    }
}

fn paint_element(
    scene: &mut PaintScene,
    elem: &WhiteboardElement,
    view: ViewTransform,
    is_dark: bool,
) {
    let screen_origin = view.world_to_screen(elem.origin);
    let scaled_width = (elem.stroke_width * view.zoom).clamp(1.0, 20.0);
    let stroke_color = if is_dark && elem.stroke_color == Color::rgb(30, 30, 30) {
        Color::rgb(224, 224, 224)
    } else {
        elem.stroke_color
    };
    let seed = elem.id.wrapping_mul(31);

    match &elem.kind {
        ElementKind::Rectangle { width, height, .. } => {
            let sw = width * view.zoom;
            let sh = height * view.zoom;
            let path = sketch_rect_path(screen_origin, sw, sh, elem.roughness, seed);
            if let Some(fill) = elem.fill_color {
                scene.fill_path(path.clone(), fill);
            }
            scene.stroke_path(path, stroke_color, scaled_width);
        }
        ElementKind::Diamond { width, height } => {
            let sw = width * view.zoom;
            let sh = height * view.zoom;
            let path = sketch_diamond_path(screen_origin, sw, sh, elem.roughness, seed);
            if let Some(fill) = elem.fill_color {
                scene.fill_path(path.clone(), fill);
            }
            scene.stroke_path(path, stroke_color, scaled_width);
        }
        ElementKind::Ellipse { radius_x, radius_y } => {
            let srx = radius_x * view.zoom;
            let sry = radius_y * view.zoom;
            let path = sketch_ellipse_path(screen_origin, srx, sry, elem.roughness, seed);
            if let Some(fill) = elem.fill_color {
                scene.fill_path(path.clone(), fill);
            }
            scene.stroke_path(path, stroke_color, scaled_width);
        }
        ElementKind::Arrow { end } => {
            let screen_end = view.world_to_screen(*end);
            let path = sketch_arrow_path(screen_origin, screen_end, elem.roughness, seed);
            scene.stroke_path(path, stroke_color, scaled_width);
        }
        ElementKind::Line { end } => {
            let screen_end = view.world_to_screen(*end);
            let path = sketch_line_path(screen_origin, screen_end, elem.roughness, seed);
            scene.stroke_path(path, stroke_color, scaled_width);
        }
        ElementKind::Freehand { points } => {
            let mut screen_pts = Vec::with_capacity(points.len() + 1);
            screen_pts.push(screen_origin);
            for p in points {
                screen_pts.push(view.world_to_screen(*p));
            }
            let path = sketch_freehand_path(&screen_pts, elem.roughness, seed);
            scene.stroke_path(path, stroke_color, scaled_width);
        }
        ElementKind::Text { content, font_size } => {
            let scaled_font_size = (font_size * view.zoom).clamp(8.0, 96.0);
            scene.draw_text(
                screen_origin,
                content.clone(),
                TextStyle {
                    font_size: scaled_font_size,
                    color: stroke_color,
                    ..Default::default()
                },
                None,
            );
        }
    }
}

fn paint_selection_halo(scene: &mut PaintScene, elem: &WhiteboardElement, view: ViewTransform) {
    let wb = elem.bounds();
    let p_min = view.world_to_screen(Point::new(wb.x, wb.y));
    let p_max = view.world_to_screen(Point::new(wb.x + wb.width, wb.y + wb.height));
    let screen_box = Rect::new(
        p_min.x - 4.0,
        p_min.y - 4.0,
        (p_max.x - p_min.x) + 8.0,
        (p_max.y - p_min.y) + 8.0,
    );

    let halo_color = Color::rgb(92, 124, 250); // Soft Excalidraw blue selection
    scene.stroke_rect(screen_box, halo_color, 1.5, CornerRadii::ZERO);

    // 4 Corner handles
    let handle_size = 8.0;
    let corners = [
        Point::new(screen_box.x, screen_box.y),
        Point::new(screen_box.x + screen_box.width, screen_box.y),
        Point::new(
            screen_box.x + screen_box.width,
            screen_box.y + screen_box.height,
        ),
        Point::new(screen_box.x, screen_box.y + screen_box.height),
    ];
    for c in corners {
        let h_rect = Rect::new(
            c.x - handle_size * 0.5,
            c.y - handle_size * 0.5,
            handle_size,
            handle_size,
        );
        scene.fill_rect(h_rect, Color::rgb(255, 255, 255), CornerRadii::ZERO);
        scene.stroke_rect(h_rect, halo_color, 1.5, CornerRadii::ZERO);
    }
}

pub fn handle_pointer(_bounds: Rect, event: &PointerEvent, ctrl: &WhiteboardController) {
    let pos = event.position;
    let view = ctrl.view.get();
    let world_pos = view.screen_to_world(pos);
    let tool = ctrl.tool.get();

    match event.phase {
        PointerPhase::Down => {
            // Space held or middle button triggers Hand pan regardless of active tool
            let is_pan_override = event.modifiers.alt
                || event.button == Some(PointerButton::Middle)
                || tool == Tool::Hand;

            if is_pan_override {
                *ctrl.interaction.borrow_mut() = InteractionState::Panning { last_screen: pos };
                return;
            }

            match tool {
                Tool::Select => {
                    let elements = ctrl.elements.get();
                    let hit_idx = elements
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, e)| e.contains_point(world_pos, 8.0))
                        .map(|(i, _)| i);

                    if let Some(idx) = hit_idx {
                        let id = elements[idx].id;
                        ctrl.selected_id.set(Some(id));
                        *ctrl.interaction.borrow_mut() = InteractionState::DraggingElement {
                            id,
                            last_world: world_pos,
                        };
                    } else {
                        ctrl.selected_id.set(None);
                    }
                }
                Tool::Eraser => {
                    let mut elements = ctrl.elements.get();
                    let initial_len = elements.len();
                    elements.retain(|e| !e.contains_point(world_pos, 10.0));
                    if elements.len() != initial_len {
                        ctrl.elements.set(elements);
                    }
                    *ctrl.interaction.borrow_mut() = InteractionState::Erasing;
                }
                Tool::Rectangle => {
                    let next_id = ctrl.next_id.get() + 1;
                    ctrl.next_id.set(next_id);
                    let draft = WhiteboardElement::new(
                        next_id,
                        world_pos,
                        ElementKind::Rectangle {
                            width: 0.0,
                            height: 0.0,
                            roundness: 0.0,
                        },
                        ctrl.stroke_color.get(),
                        None,
                        ctrl.stroke_width.get(),
                    );
                    *ctrl.interaction.borrow_mut() = InteractionState::Drawing { draft };
                }
                Tool::Diamond => {
                    let next_id = ctrl.next_id.get() + 1;
                    ctrl.next_id.set(next_id);
                    let draft = WhiteboardElement::new(
                        next_id,
                        world_pos,
                        ElementKind::Diamond {
                            width: 0.0,
                            height: 0.0,
                        },
                        ctrl.stroke_color.get(),
                        None,
                        ctrl.stroke_width.get(),
                    );
                    *ctrl.interaction.borrow_mut() = InteractionState::Drawing { draft };
                }
                Tool::Ellipse => {
                    let next_id = ctrl.next_id.get() + 1;
                    ctrl.next_id.set(next_id);
                    let draft = WhiteboardElement::new(
                        next_id,
                        world_pos,
                        ElementKind::Ellipse {
                            radius_x: 0.0,
                            radius_y: 0.0,
                        },
                        ctrl.stroke_color.get(),
                        None,
                        ctrl.stroke_width.get(),
                    );
                    *ctrl.interaction.borrow_mut() = InteractionState::Drawing { draft };
                }
                Tool::Arrow => {
                    let next_id = ctrl.next_id.get() + 1;
                    ctrl.next_id.set(next_id);
                    let draft = WhiteboardElement::new(
                        next_id,
                        world_pos,
                        ElementKind::Arrow { end: world_pos },
                        ctrl.stroke_color.get(),
                        None,
                        ctrl.stroke_width.get(),
                    );
                    *ctrl.interaction.borrow_mut() = InteractionState::Drawing { draft };
                }
                Tool::Line => {
                    let next_id = ctrl.next_id.get() + 1;
                    ctrl.next_id.set(next_id);
                    let draft = WhiteboardElement::new(
                        next_id,
                        world_pos,
                        ElementKind::Line { end: world_pos },
                        ctrl.stroke_color.get(),
                        None,
                        ctrl.stroke_width.get(),
                    );
                    *ctrl.interaction.borrow_mut() = InteractionState::Drawing { draft };
                }
                Tool::Pen => {
                    let next_id = ctrl.next_id.get() + 1;
                    ctrl.next_id.set(next_id);
                    let draft = WhiteboardElement::new(
                        next_id,
                        world_pos,
                        ElementKind::Freehand {
                            points: vec![world_pos],
                        },
                        ctrl.stroke_color.get(),
                        None,
                        ctrl.stroke_width.get(),
                    );
                    *ctrl.interaction.borrow_mut() = InteractionState::Drawing { draft };
                }
                Tool::Text => {
                    let next_id = ctrl.next_id.get() + 1;
                    ctrl.next_id.set(next_id);
                    let new_elem = WhiteboardElement::new(
                        next_id,
                        world_pos,
                        ElementKind::Text {
                            content: "Type note here...".to_string(),
                            font_size: 20.0,
                        },
                        ctrl.stroke_color.get(),
                        None,
                        1.0,
                    );
                    let mut list = ctrl.elements.get();
                    ctrl.history.borrow_mut().push(list.clone());
                    list.push(new_elem);
                    ctrl.elements.set(list);
                    ctrl.selected_id.set(Some(next_id));
                    *ctrl.interaction.borrow_mut() = InteractionState::Idle;
                }
                Tool::Hand => {
                    *ctrl.interaction.borrow_mut() = InteractionState::Panning { last_screen: pos };
                }
            }
        }
        PointerPhase::Move => {
            let mut state = ctrl.interaction.borrow_mut();
            match &mut *state {
                InteractionState::Panning { last_screen } => {
                    let delta = Point::new(pos.x - last_screen.x, pos.y - last_screen.y);
                    *last_screen = pos;
                    let mut v = ctrl.view.get();
                    v.pan.x += delta.x;
                    v.pan.y += delta.y;
                    ctrl.view.set(v);
                }
                InteractionState::DraggingElement { id, last_world } => {
                    let delta = Point::new(world_pos.x - last_world.x, world_pos.y - last_world.y);
                    *last_world = world_pos;
                    let mut elems = ctrl.elements.get();
                    if let Some(target) = elems.iter_mut().find(|e| e.id == *id) {
                        target.translate(delta);
                        ctrl.elements.set(elems);
                    }
                }
                InteractionState::Drawing { draft } => match &mut draft.kind {
                    ElementKind::Rectangle { width, height, .. } => {
                        *width = world_pos.x - draft.origin.x;
                        *height = world_pos.y - draft.origin.y;
                    }
                    ElementKind::Diamond { width, height } => {
                        *width = world_pos.x - draft.origin.x;
                        *height = world_pos.y - draft.origin.y;
                    }
                    ElementKind::Ellipse { radius_x, radius_y } => {
                        *radius_x = (world_pos.x - draft.origin.x).abs();
                        *radius_y = (world_pos.y - draft.origin.y).abs();
                    }
                    ElementKind::Arrow { end } | ElementKind::Line { end } => {
                        *end = world_pos;
                    }
                    ElementKind::Freehand { points } => {
                        if let Some(last) = points.last()
                            && last.distance_to(world_pos) > 2.0
                        {
                            points.push(world_pos);
                        }
                    }
                    _ => {}
                },
                InteractionState::Erasing => {
                    let mut elems = ctrl.elements.get();
                    let before_len = elems.len();
                    elems.retain(|e| !e.contains_point(world_pos, 10.0));
                    if elems.len() != before_len {
                        ctrl.elements.set(elems);
                    }
                }
                InteractionState::Idle => {}
            }
        }
        PointerPhase::Up | PointerPhase::Cancel => {
            let mut state = ctrl.interaction.borrow_mut();
            match std::mem::replace(&mut *state, InteractionState::Idle) {
                InteractionState::Drawing { draft } => {
                    let is_valid = match &draft.kind {
                        ElementKind::Rectangle { width, height, .. }
                        | ElementKind::Diamond { width, height } => {
                            width.abs() > 4.0 || height.abs() > 4.0
                        }
                        ElementKind::Ellipse { radius_x, radius_y } => {
                            *radius_x > 3.0 || *radius_y > 3.0
                        }
                        ElementKind::Arrow { end } | ElementKind::Line { end } => {
                            draft.origin.distance_to(*end) > 4.0
                        }
                        ElementKind::Freehand { points } => points.len() > 1,
                        ElementKind::Text { .. } => true,
                    };

                    if is_valid {
                        let mut current = ctrl.elements.get();
                        ctrl.history.borrow_mut().push(current.clone());
                        let draft_id = draft.id;
                        current.push(draft);
                        ctrl.elements.set(current);
                        ctrl.selected_id.set(Some(draft_id));
                        if !ctrl.tool_locked.get() {
                            ctrl.tool.set(Tool::Select);
                        }
                    }
                }
                InteractionState::DraggingElement { .. } | InteractionState::Erasing => {
                    // Push current snapshot into history
                    let current = ctrl.elements.get();
                    ctrl.history.borrow_mut().push(current);
                }
                _ => {}
            }
        }
    }
}
