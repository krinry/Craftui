use crate::bridge::A11yBridge;
use crate::types::{A11yAction, A11yMetadata, A11yRole, A11ySnapshotNode};
use craftui_core::{NodeId, Rect};
use slotmap::SlotMap;

#[test]
fn tree_update_and_action_routing() {
    let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
    let root = nodes.insert(());
    let btn = nodes.insert(());

    let mut bridge = A11yBridge::new("Test App");
    let update = bridge.build_tree_update(
        root,
        Some(btn),
        &[
            A11ySnapshotNode {
                id: root,
                bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
                meta: A11yMetadata::default(),
                children: vec![btn],
            },
            A11ySnapshotNode {
                id: btn,
                bounds: Rect::new(20.0, 20.0, 120.0, 40.0),
                meta: A11yMetadata {
                    role: A11yRole::Button,
                    label: Some("Submit".into()),
                    focusable: true,
                    ..Default::default()
                },
                children: vec![],
            },
        ],
    );

    assert_eq!(update.nodes.len(), 2);
    assert_eq!(update.focus, A11yBridge::to_ak_id(btn));

    let req = accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_tree: accesskit::TreeId::ROOT,
        target_node: A11yBridge::to_ak_id(btn),
        data: None,
    };
    assert_eq!(bridge.translate_action(&req), Some(A11yAction::Click(btn)));
}

#[test]
fn incremental_a11y_update_skips_unchanged_and_diffs_deltas() {
    let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
    let root = nodes.insert(());
    let btn = nodes.insert(());
    let lbl = nodes.insert(());

    let mut bridge = A11yBridge::new("Incremental Test");
    let mut snaps = vec![
        A11ySnapshotNode {
            id: root,
            bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
            meta: A11yMetadata::default(),
            children: vec![btn, lbl],
        },
        A11ySnapshotNode {
            id: btn,
            bounds: Rect::new(20.0, 20.0, 120.0, 40.0),
            meta: A11yMetadata {
                role: A11yRole::Button,
                label: Some("Save".into()),
                focusable: true,
                ..Default::default()
            },
            children: vec![],
        },
        A11ySnapshotNode {
            id: lbl,
            bounds: Rect::new(20.0, 80.0, 100.0, 20.0),
            meta: A11yMetadata {
                role: A11yRole::Label,
                label: Some("Ready".into()),
                ..Default::default()
            },
            children: vec![],
        },
    ];

    let u1 = bridge
        .build_incremental_update(root, Some(btn), &snaps)
        .expect("first build emits full tree");
    assert_eq!(u1.nodes.len(), 3);
    assert!(u1.tree.is_some());

    // Identical frame -> None!
    assert!(
        bridge
            .build_incremental_update(root, Some(btn), &snaps)
            .is_none()
    );

    // Update only one node label -> only 1 node serialized!
    snaps[2].meta.label = Some("Saved!".into());
    let u2 = bridge
        .build_incremental_update(root, Some(btn), &snaps)
        .expect("label mutation emits delta");
    assert_eq!(u2.nodes.len(), 1);
    assert_eq!(u2.nodes[0].0, A11yBridge::to_ak_id(lbl));
    assert!(u2.tree.is_none());
}
