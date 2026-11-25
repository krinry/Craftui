use crate::ast::*;

/// Prunes redundant structural container wrappers that add no layout, styling,
/// borders, paddings, or event handlers (clean-room equivalent to Slint's rectangle optimizer).
pub fn prune_dead_containers(file: &mut CuiFile) {
    for comp in &mut file.components {
        if let Some(ref mut root) = comp.root {
            prune_element(root);
        }
    }
}

fn prune_element(el: &mut ElementNode) {
    // 1. First recurse into all children
    for child in &mut el.children {
        match child {
            ChildItem::Element(inner) => prune_element(inner),
            ChildItem::For { body, .. } => prune_element(body),
            ChildItem::If {
                then_branch,
                else_branch,
                ..
            } => {
                prune_element(then_branch);
                if let Some(else_b) = else_branch {
                    prune_element(else_b);
                }
            }
            ChildItem::ChildrenSlot { .. } => {}
        }
    }

    // 2. Check each direct child element: if it is a useless container wrapping a single element, unwrap it
    let mut new_children = Vec::with_capacity(el.children.len());
    for child in el.children.drain(..) {
        match child {
            ChildItem::Element(mut inner) => {
                if is_useless_container(&inner)
                    && inner.children.len() == 1
                    && !matches!(inner.children.first(), Some(ChildItem::ChildrenSlot { .. }))
                {
                    let single_grandchild = inner.children.pop().unwrap();
                    new_children.push(single_grandchild);
                } else {
                    new_children.push(ChildItem::Element(inner));
                }
            }
            other => new_children.push(other),
        }
    }
    el.children = new_children;
}

fn is_useless_container(el: &ElementNode) -> bool {
    // Only generic containers or columns/rows are candidates
    if !matches!(el.kind.as_str(), "Container") {
        return false;
    }

    // Must have no handlers, no animations, no states, no transitions
    if !el.handlers.is_empty()
        || !el.animations.is_empty()
        || !el.states.is_empty()
        || !el.transitions.is_empty()
    {
        return false;
    }

    // Must have no styling properties (background, border, padding, margin, shadow, etc.)
    if !el.props.is_empty() {
        return false;
    }

    true
}
