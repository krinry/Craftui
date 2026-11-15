use crate::ast::*;

/// Clean-room state lowering pass:
/// Lowers declarative `states [ ... ]` and `transitions [ ... ]` on elements
/// into reactive ternary expressions and animations.
pub fn lower_states(file: &mut CuiFile) {
    for comp in &mut file.components {
        if let Some(ref mut root) = comp.root {
            lower_states_in_element(root);
        }
    }
}

fn lower_states_in_element(el: &mut ElementNode) {
    // Recurse into children first
    for child in &mut el.children {
        match child {
            ChildItem::Element(inner) => lower_states_in_element(inner),
            ChildItem::For { body, .. } => lower_states_in_element(body),
            ChildItem::If {
                then_branch,
                else_branch,
                ..
            } => {
                lower_states_in_element(then_branch);
                if let Some(else_b) = else_branch {
                    lower_states_in_element(else_b);
                }
            }
        }
    }

    if el.states.is_empty() && el.transitions.is_empty() {
        return;
    }

    // 1. Lower transitions: preserve from_state and to_state selectors on animations
    for tr in el.transitions.drain(..) {
        for mut anim in tr.animations {
            if let Some(ref from) = tr.from_state {
                anim.fields
                    .push(("from_state".into(), Expr::String(from.clone())));
            }
            if let Some(ref to) = tr.to_state {
                anim.fields
                    .push(("to_state".into(), Expr::String(to.clone())));
            }
            let duplicate = el
                .animations
                .iter()
                .any(|a| a.prop == anim.prop && a.fields == anim.fields);
            if !duplicate {
                el.animations.push(anim);
            }
        }
    }

    if el.states.is_empty() {
        return;
    }

    // 2. Collect unique property names overridden in states
    let mut overridden_prop_names: Vec<String> = Vec::new();
    for state in &el.states {
        for prop in &state.property_overrides {
            if !overridden_prop_names.contains(&prop.name) {
                overridden_prop_names.push(prop.name.clone());
            }
        }
    }

    // 3. For each overridden property, construct the conditional expression chain:
    // If state1.cond then state1.val else (if state2.cond then state2.val else base_val)
    for prop_name in overridden_prop_names {
        // Find existing base property value, or default to standard property default
        let base_val = el
            .props
            .iter()
            .find(|p| p.name == prop_name)
            .map(|p| p.value.clone())
            .unwrap_or_else(|| default_prop_value(&prop_name));

        // Fold in reverse so the first state declared takes highest priority
        let mut folded = base_val;
        for state in el.states.iter().rev() {
            if let Some(ov) = state
                .property_overrides
                .iter()
                .find(|p| p.name == prop_name)
            {
                folded = Expr::Ternary {
                    cond: Box::new(state.condition.clone()),
                    then_expr: Box::new(ov.value.clone()),
                    else_expr: Box::new(folded),
                };
            }
        }

        if let Some(existing) = el.props.iter_mut().find(|p| p.name == prop_name) {
            existing.value = folded;
        } else {
            el.props.push(PropBinding {
                name: prop_name,
                two_way: false,
                value: folded,
                span: el.span,
            });
        }
    }

    // States are now fully lowered into reactive ternary expressions!
    el.states.clear();
}

fn default_prop_value(name: &str) -> Expr {
    match name {
        "opacity" => Expr::Float(1.0),
        "visible" => Expr::Bool(true),
        "background" | "color" | "border_color" => Expr::ColorHex("00000000".into()),
        "width" | "height" => Expr::Ident("Auto".into()),
        _ => Expr::Float(0.0),
    }
}
