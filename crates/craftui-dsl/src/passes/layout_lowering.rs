use crate::ast::*;

/// Layout lowering and property normalization pass:
/// Deduplicates property definitions, normalizes dimensions, and prepares
/// elements for high-performance layout computation.
pub fn lower_layout(file: &mut CuiFile) {
    for comp in &mut file.components {
        if let Some(ref mut root) = comp.root {
            lower_layout_element(root);
        }
    }
}

fn lower_layout_element(el: &mut ElementNode) {
    // 1. Recurse down element tree
    for child in &mut el.children {
        match child {
            ChildItem::Element(inner) => lower_layout_element(inner),
            ChildItem::For { body, .. } => lower_layout_element(body),
            ChildItem::If {
                then_branch,
                else_branch,
                ..
            } => {
                lower_layout_element(then_branch);
                if let Some(else_b) = else_branch {
                    lower_layout_element(else_b);
                }
            }
            ChildItem::ChildrenSlot { .. } => {}
        }
    }

    // 2. Deduplicate properties in reverse so the last definition wins
    let mut seen = std::collections::HashSet::new();
    let mut deduped_props = Vec::with_capacity(el.props.len());
    for prop in el.props.drain(..).rev() {
        if seen.insert(prop.name.clone()) {
            deduped_props.push(prop);
        }
    }
    deduped_props.reverse();
    el.props = deduped_props;
}
