//! Layout results for the common arrangements, through the public API.

use craftui_core::{Alignment, Direction, Insets, Justify, NodeId, Rect, Size, dp, pct};
use craftui_layout::{LayoutEngine, LayoutStyle};
use slotmap::SlotMap;

/// Builds a tree of layout nodes and computes it.
struct Fixture {
    ids: SlotMap<NodeId, ()>,
    engine: LayoutEngine,
}

impl Fixture {
    fn new() -> Self {
        Self {
            ids: SlotMap::with_key(),
            engine: LayoutEngine::new(),
        }
    }

    fn node(&mut self, style: LayoutStyle) -> NodeId {
        let id = self.ids.insert(());
        self.engine.upsert_node(id, style);
        id
    }

    fn parent(&mut self, style: LayoutStyle, children: &[NodeId]) -> NodeId {
        let id = self.node(style);
        self.engine.set_children(id, children);
        id
    }

    fn compute(&mut self, root: NodeId, width: f32, height: f32) {
        self.engine.compute_layout(
            root,
            width,
            height,
            Insets::ZERO,
            1.0,
            Direction::Ltr,
            |_, _| Size::ZERO,
        );
    }

    fn rect(&self, node: NodeId) -> Rect {
        self.engine.rect(node)
    }
}

fn boxed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::column().with_size(dp(width), dp(height))
}

#[test]
fn a_column_stacks_children_with_padding_and_gap() {
    let mut f = Fixture::new();
    let a = f.node(LayoutStyle::column().with_height(dp(20.0)));
    let b = f.node(LayoutStyle::column().with_height(dp(30.0)));
    let root = f.parent(
        LayoutStyle::column()
            .with_size(dp(200.0), dp(300.0))
            .with_padding(Insets::all(10.0))
            .with_gap(5.0),
        &[a, b],
    );
    f.compute(root, 200.0, 300.0);

    // Children without a width stretch across the column.
    assert_eq!(f.rect(a), Rect::new(10.0, 10.0, 180.0, 20.0));
    assert_eq!(f.rect(b), Rect::new(10.0, 35.0, 180.0, 30.0));
}

#[test]
fn a_row_places_children_side_by_side_and_centres_them_vertically() {
    let mut f = Fixture::new();
    let a = f.node(boxed(50.0, 20.0));
    let b = f.node(boxed(70.0, 40.0));
    let root = f.parent(
        LayoutStyle::row()
            .with_size(dp(300.0), dp(60.0))
            .with_gap(10.0),
        &[a, b],
    );
    f.compute(root, 300.0, 60.0);

    assert_eq!(f.rect(a), Rect::new(0.0, 20.0, 50.0, 20.0));
    assert_eq!(f.rect(b), Rect::new(60.0, 10.0, 70.0, 40.0));
}

#[test]
fn justify_distributes_children_along_the_main_axis() {
    // Three 50dp children in a 390dp row leave 240dp to distribute.
    let cases = [
        (Justify::Start, [0.0, 50.0, 100.0]),
        (Justify::Center, [120.0, 170.0, 220.0]),
        (Justify::End, [240.0, 290.0, 340.0]),
        (Justify::SpaceBetween, [0.0, 170.0, 340.0]),
        (Justify::SpaceAround, [40.0, 170.0, 300.0]),
        (Justify::SpaceEvenly, [60.0, 170.0, 280.0]),
    ];
    for (justify, expected) in cases {
        let mut f = Fixture::new();
        let children = [
            f.node(boxed(50.0, 20.0)),
            f.node(boxed(50.0, 20.0)),
            f.node(boxed(50.0, 20.0)),
        ];
        let root = f.parent(
            LayoutStyle::row()
                .with_size(dp(390.0), dp(20.0))
                .with_justify(justify),
            &children,
        );
        f.compute(root, 390.0, 20.0);

        let xs = children.map(|c| f.rect(c).x);
        assert_eq!(xs, expected, "{justify:?}");
    }
}

#[test]
fn align_positions_children_across_a_column() {
    let cases = [
        (Alignment::Start, 0.0, 40.0),
        (Alignment::Center, 30.0, 40.0),
        (Alignment::End, 60.0, 40.0),
    ];
    for (align, x, width) in cases {
        let mut f = Fixture::new();
        let child = f.node(boxed(40.0, 10.0));
        let root = f.parent(
            LayoutStyle::column()
                .with_size(dp(100.0), dp(50.0))
                .with_align(align),
            &[child],
        );
        f.compute(root, 100.0, 50.0);
        assert_eq!(f.rect(child), Rect::new(x, 0.0, width, 10.0), "{align:?}");
    }

    // With `Stretch`, a child without a width fills the column.
    let mut f = Fixture::new();
    let child = f.node(LayoutStyle::column().with_height(dp(10.0)));
    let root = f.parent(
        LayoutStyle::column()
            .with_size(dp(100.0), dp(50.0))
            .with_align(Alignment::Stretch),
        &[child],
    );
    f.compute(root, 100.0, 50.0);
    assert_eq!(f.rect(child).width, 100.0);
}

#[test]
fn flex_grow_shares_spare_space_in_proportion() {
    let mut f = Fixture::new();
    let fixed = f.node(boxed(60.0, 20.0));
    let one = f.node(LayoutStyle::column().with_flex_grow(1.0));
    let two = f.node(LayoutStyle::column().with_flex_grow(2.0));
    let root = f.parent(
        LayoutStyle::row().with_size(dp(360.0), dp(20.0)),
        &[fixed, one, two],
    );
    f.compute(root, 360.0, 20.0);

    assert_eq!(f.rect(fixed).width, 60.0);
    assert_eq!(f.rect(one).width, 100.0);
    assert_eq!(f.rect(two).width, 200.0);
    assert_eq!(f.rect(two).x, 160.0);
}

#[test]
fn percentages_resolve_against_the_parent() {
    let mut f = Fixture::new();
    let child = f.node(LayoutStyle::column().with_size(pct(50.0), pct(25.0)));
    let root = f.parent(
        LayoutStyle::column().with_size(dp(400.0), dp(200.0)),
        &[child],
    );
    f.compute(root, 400.0, 200.0);

    let rect = f.rect(child);
    assert_eq!((rect.width, rect.height), (200.0, 50.0));
}

#[test]
fn margin_moves_a_child_and_keeps_its_size() {
    let mut f = Fixture::new();
    let child = f.node(boxed(40.0, 20.0).with_margin(Insets::new(5.0, 0.0, 0.0, 15.0)));
    let after = f.node(boxed(40.0, 20.0));
    let root = f.parent(
        LayoutStyle::column()
            .with_size(dp(200.0), dp(200.0))
            .with_align(Alignment::Start),
        &[child, after],
    );
    f.compute(root, 200.0, 200.0);

    assert_eq!(f.rect(child), Rect::new(15.0, 5.0, 40.0, 20.0));
    assert_eq!(f.rect(after).y, 25.0);
}

#[test]
fn a_hidden_child_takes_no_space() {
    let mut f = Fixture::new();
    let first = f.node(LayoutStyle::column().with_height(dp(20.0)));
    let mut hidden_style = LayoutStyle::column().with_height(dp(20.0));
    hidden_style.hidden = true;
    let hidden = f.node(hidden_style);
    let last = f.node(LayoutStyle::column().with_height(dp(20.0)));
    let root = f.parent(
        LayoutStyle::column()
            .with_size(dp(100.0), dp(100.0))
            .with_gap(4.0),
        &[first, hidden, last],
    );
    f.compute(root, 100.0, 100.0);

    assert_eq!(f.rect(last).y, 24.0, "no gap is left for the hidden child");

    // Showing it again puts it back in the flow.
    f.engine
        .upsert_node(hidden, LayoutStyle::column().with_height(dp(20.0)));
    f.compute(root, 100.0, 100.0);
    assert_eq!(f.rect(hidden).y, 24.0);
    assert_eq!(f.rect(last).y, 48.0);
}

#[test]
fn a_leaf_is_sized_by_its_measure_function() {
    let mut ids: SlotMap<NodeId, ()> = SlotMap::with_key();
    let label = ids.insert(());
    let root = ids.insert(());
    let mut engine = LayoutEngine::new();
    engine.upsert_node(label, LayoutStyle::column());
    engine.upsert_node(
        root,
        LayoutStyle::column()
            .with_size(dp(300.0), dp(100.0))
            .with_align(Alignment::Start),
    );
    engine.set_children(root, &[label]);

    engine.compute_layout(
        root,
        300.0,
        100.0,
        Insets::ZERO,
        1.0,
        Direction::Ltr,
        |node, _| {
            if node == label {
                Size::new(72.0, 18.0)
            } else {
                Size::ZERO
            }
        },
    );

    assert_eq!(engine.rect(label), Rect::new(0.0, 0.0, 72.0, 18.0));
}

#[test]
fn removing_a_child_closes_the_gap() {
    let mut f = Fixture::new();
    let a = f.node(LayoutStyle::column().with_height(dp(20.0)));
    let b = f.node(LayoutStyle::column().with_height(dp(20.0)));
    let c = f.node(LayoutStyle::column().with_height(dp(20.0)));
    let root = f.parent(
        LayoutStyle::column().with_size(dp(100.0), dp(100.0)),
        &[a, b, c],
    );
    f.compute(root, 100.0, 100.0);
    assert_eq!(f.rect(c).y, 40.0);

    f.engine.set_children(root, &[a, c]);
    f.engine.remove_node(b);
    f.compute(root, 100.0, 100.0);
    assert_eq!(f.rect(c).y, 20.0);
}

#[test]
fn a_new_window_size_is_applied_on_the_next_layout() {
    let mut f = Fixture::new();
    let child = f.node(LayoutStyle::column().with_flex_grow(1.0));
    let root = f.parent(
        LayoutStyle::column().with_size(pct(100.0), pct(100.0)),
        &[child],
    );

    f.compute(root, 400.0, 800.0);
    assert_eq!(f.rect(child), Rect::new(0.0, 0.0, 400.0, 800.0));

    f.compute(root, 1100.0, 720.0);
    assert_eq!(f.rect(child), Rect::new(0.0, 0.0, 1100.0, 720.0));
}

/// A header, then an area holding content much taller than the window.
fn header_and_tall_area(area: LayoutStyle) -> (Fixture, NodeId, NodeId) {
    let mut f = Fixture::new();
    let header = f.node(LayoutStyle::column().with_height(dp(50.0)));
    let content = f.node(LayoutStyle::column().with_height(dp(1000.0)));
    let area = f.parent(area, &[content]);
    let root = f.parent(
        LayoutStyle::column().with_size(dp(200.0), dp(300.0)),
        &[header, area],
    );
    f.compute(root, 200.0, 300.0);
    (f, header, area)
}

#[test]
fn fill_remaining_keeps_a_tall_area_inside_its_parent() {
    let (f, header, area) = header_and_tall_area(LayoutStyle::column().fill_remaining(1.0));

    assert_eq!(f.rect(header).height, 50.0, "the header keeps its size");
    assert_eq!(f.rect(area), Rect::new(0.0, 50.0, 200.0, 250.0));
}

#[test]
fn plain_flex_grow_never_makes_an_area_smaller_than_its_content() {
    // This is the standard flexbox rule, and the reason `fill_remaining`
    // exists: the area is pushed past the bottom of its 300dp parent.
    let (f, _header, area) = header_and_tall_area(LayoutStyle::column().with_flex_grow(1.0));

    let rect = f.rect(area);
    assert_eq!(rect.height, 1000.0);
    assert!(rect.y + rect.height > 300.0);
}
