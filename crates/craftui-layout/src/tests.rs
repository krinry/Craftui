use craftui_core::{Direction, Insets, NodeId, Rect, Size, dp, pct};
use slotmap::SlotMap;

use crate::engine::LayoutEngine;
use crate::style::LayoutStyle;

#[test]
fn flex_row_and_safe_area_and_rtl() {
    let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
    let root = nodes.insert(());
    let a = nodes.insert(());
    let b = nodes.insert(());

    let mut engine = LayoutEngine::new();
    engine.upsert_node(
        root,
        LayoutStyle::row()
            .with_size(dp(300.0), dp(100.0))
            .with_gap(10.0)
            .with_safe_area(true),
    );
    engine.upsert_node(a, LayoutStyle::column().with_size(dp(80.0), pct(100.0)));
    engine.upsert_node(
        b,
        LayoutStyle::column()
            .with_flex_grow(1.0)
            .with_height(pct(100.0)),
    );
    engine.set_children(root, &[a, b]);

    let safe = Insets::new(20.0, 0.0, 10.0, 16.0);
    engine.compute_layout(root, 300.0, 100.0, safe, 1.0, Direction::Ltr, |_, _| {
        Size::ZERO
    });

    let ra = engine.rect(a);
    let rb = engine.rect(b);
    assert_eq!(ra.x, 16.0);
    assert_eq!(ra.y, 20.0);
    assert_eq!(ra.width, 80.0);
    assert_eq!(ra.height, 70.0);
    assert_eq!(rb.x, 106.0);
    assert_eq!(rb.width, 194.0);

    // Verify RTL mirroring swaps a and b horizontally!
    engine.compute_layout(
        root,
        300.0,
        100.0,
        Insets::ZERO,
        1.0,
        Direction::Rtl,
        |_, _| Size::ZERO,
    );
    let ra_rtl = engine.rect(a);
    let rb_rtl = engine.rect(b);
    assert!(
        ra_rtl.x > rb_rtl.x,
        "RTL should place first child on the right"
    );
}

#[test]
fn grid_layout_and_leaf_measure() {
    let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
    let grid = nodes.insert(());
    let c1 = nodes.insert(());
    let c2 = nodes.insert(());

    let mut engine = LayoutEngine::new();
    engine.upsert_node(
        grid,
        LayoutStyle::grid(2).with_width(dp(220.0)).with_gap(20.0),
    );
    engine.upsert_node(c1, LayoutStyle::column());
    engine.upsert_node(c2, LayoutStyle::column());
    engine.set_children(grid, &[c1, c2]);

    engine.compute_layout(
        grid,
        220.0,
        400.0,
        Insets::ZERO,
        1.0,
        Direction::Ltr,
        |_, _| Size::new(50.0, 40.0),
    );
    assert_eq!(engine.rect(c1), Rect::new(0.0, 0.0, 100.0, 40.0));
    assert_eq!(engine.rect(c2), Rect::new(120.0, 0.0, 100.0, 40.0));
}

#[test]
fn header_top_inset_combines_with_padding_and_safe_area() {
    let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
    let card = nodes.insert(());
    let child = nodes.insert(());

    let mut engine = LayoutEngine::new();
    let mut card_style = LayoutStyle::column()
        .with_width(dp(240.0))
        .with_padding(Insets::all(10.0))
        .with_safe_area(true);
    card_style.header_top_inset = 44.0;
    engine.upsert_node(card, card_style);
    engine.upsert_node(child, LayoutStyle::column().with_height(dp(30.0)));
    engine.set_children(card, &[child]);

    let safe = Insets::new(12.0, 0.0, 8.0, 4.0);
    engine.compute_layout(card, 240.0, 300.0, safe, 1.0, Direction::Ltr, |_, _| {
        Size::ZERO
    });

    let rc = engine.rect(child);
    assert_eq!(rc.x, 14.0);
    assert_eq!(rc.y, 10.0 + 12.0 + 44.0);
    assert_eq!(
        engine.rect(card).height,
        10.0 + 12.0 + 44.0 + 30.0 + 10.0 + 8.0
    );
}
