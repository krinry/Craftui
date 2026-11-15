use crate::ast::*;

/// Multi-pass constant folding and expression propagation:
/// Evaluates compile-time constant arithmetic, string concatenations,
/// boolean logic, and constant ternary branches.
pub fn propagate_constants(file: &mut CuiFile) {
    for comp in &mut file.components {
        for prop in &mut comp.properties {
            if let Some(ref mut def) = prop.default_value {
                fold_expr(def);
            }
        }
        if let Some(ref mut root) = comp.root {
            fold_element(root);
        }
    }
}

fn fold_element(el: &mut ElementNode) {
    for prop in &mut el.props {
        fold_expr(&mut prop.value);
    }
    for handler in &mut el.handlers {
        for stmt in &mut handler.body {
            fold_stmt(stmt);
        }
    }
    for child in &mut el.children {
        match child {
            ChildItem::Element(inner) => fold_element(inner),
            ChildItem::For {
                iterable,
                key,
                body,
                ..
            } => {
                fold_expr(iterable);
                if let Some(k) = key {
                    fold_expr(k);
                }
                fold_element(body);
            }
            ChildItem::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                fold_expr(condition);
                fold_element(then_branch);
                if let Some(else_b) = else_branch {
                    fold_element(else_b);
                }
            }
        }
    }
}

fn fold_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            fold_expr(target);
            fold_expr(value);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            fold_expr(condition);
            for s in then_branch {
                fold_stmt(s);
            }
            for s in else_branch {
                fold_stmt(s);
            }
        }
        Stmt::Call { args, .. } => {
            for arg in args {
                fold_expr(arg);
            }
        }
        Stmt::Expr(e) => fold_expr(e),
    }
}

pub fn fold_expr(expr: &mut Expr) {
    // 1. Recurse down sub-expressions first
    match expr {
        Expr::Member(base, field) => {
            fold_expr(base);
            if let Expr::Ident(id) = base.as_ref()
                && id == "Math"
            {
                match field.as_str() {
                    "PI" => {
                        *expr = Expr::Float(std::f64::consts::PI);
                    }
                    "E" => {
                        *expr = Expr::Float(std::f64::consts::E);
                    }
                    "LN2" => {
                        *expr = Expr::Float(std::f64::consts::LN_2);
                    }
                    "LN10" => {
                        *expr = Expr::Float(std::f64::consts::LN_10);
                    }
                    "LOG2E" => {
                        *expr = Expr::Float(std::f64::consts::LOG2_E);
                    }
                    "LOG10E" => {
                        *expr = Expr::Float(std::f64::consts::LOG10_E);
                    }
                    "SQRT2" => {
                        *expr = Expr::Float(std::f64::consts::SQRT_2);
                    }
                    _ => {}
                }
            }
        }
        Expr::Index(base, idx) => {
            fold_expr(base);
            fold_expr(idx);
        }
        Expr::Range { start, end, .. } => {
            fold_expr(start);
            fold_expr(end);
        }
        Expr::StructLit { fields, .. } => {
            for (_, val) in fields {
                fold_expr(val);
            }
        }
        Expr::Array(items) => {
            for item in items {
                fold_expr(item);
            }
        }
        Expr::Call(name, args) => {
            for arg in args.iter_mut() {
                fold_expr(arg);
            }
            if let Some(folded) = fold_math_call(name, args) {
                *expr = folded;
            }
        }
        Expr::MethodCall(recv, method, args) => {
            fold_expr(recv);
            for arg in args.iter_mut() {
                fold_expr(arg);
            }
            if let Expr::Ident(id) = recv.as_ref()
                && id == "Math"
                && let Some(folded) = fold_math_call(method, args)
            {
                *expr = folded;
                return;
            }
            if let Some(v) = get_num(recv) {
                if args.is_empty() {
                    match method.as_str() {
                        "sin" => {
                            *expr = Expr::Float(v.sin());
                        }
                        "cos" => {
                            *expr = Expr::Float(v.cos());
                        }
                        "tan" => {
                            *expr = Expr::Float(v.tan());
                        }
                        "sqrt" => {
                            *expr = Expr::Float(v.sqrt());
                        }
                        "abs" => {
                            *expr = Expr::Float(v.abs());
                        }
                        "round" => {
                            *expr = Expr::Float(v.round());
                        }
                        "floor" => {
                            *expr = Expr::Float(v.floor());
                        }
                        "ceil" => {
                            *expr = Expr::Float(v.ceil());
                        }
                        _ => {}
                    }
                } else if args.len() == 1 {
                    if let Some(o) = get_num(&args[0]) {
                        match method.as_str() {
                            "min" => {
                                *expr = Expr::Float(v.min(o));
                            }
                            "max" => {
                                *expr = Expr::Float(v.max(o));
                            }
                            "pow" => {
                                *expr = Expr::Float(v.powf(o));
                            }
                            _ => {}
                        }
                    }
                } else if args.len() == 2
                    && let (Some(lo), Some(hi)) = (get_num(&args[0]), get_num(&args[1]))
                    && method == "clamp"
                {
                    let hi_c = hi.max(lo);
                    *expr = Expr::Float(v.clamp(lo, hi_c));
                }
            }
        }
        Expr::UnaryNeg(inner) => {
            fold_expr(inner);
            if let Expr::Int(v) = **inner {
                *expr = Expr::Int(-v);
                return;
            }
            if let Expr::Float(v) = **inner {
                *expr = Expr::Float(-v);
            }
        }
        Expr::UnaryNot(inner) => {
            fold_expr(inner);
            if let Expr::Bool(b) = **inner {
                *expr = Expr::Bool(!b);
            }
        }
        Expr::Binary(left, op, right) => {
            fold_expr(left);
            fold_expr(right);
            if let Some(folded) = fold_binary(left, *op, right) {
                *expr = folded;
            }
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            fold_expr(cond);
            fold_expr(then_expr);
            fold_expr(else_expr);
            if let Expr::Bool(b) = **cond {
                if b {
                    *expr = (**then_expr).clone();
                } else {
                    *expr = (**else_expr).clone();
                }
            }
        }
        _ => {}
    }
}

fn fold_binary(left: &Expr, op: BinOp, right: &Expr) -> Option<Expr> {
    match (left, op, right) {
        // Integer arithmetic
        (Expr::Int(a), BinOp::Add, Expr::Int(b)) => Some(Expr::Int(a + b)),
        (Expr::Int(a), BinOp::Sub, Expr::Int(b)) => Some(Expr::Int(a - b)),
        (Expr::Int(a), BinOp::Mul, Expr::Int(b)) => Some(Expr::Int(a * b)),
        (Expr::Int(a), BinOp::Div, Expr::Int(b)) if *b != 0 => Some(Expr::Int(a / b)),

        // Float arithmetic
        (Expr::Float(a), BinOp::Add, Expr::Float(b)) => Some(Expr::Float(a + b)),
        (Expr::Float(a), BinOp::Sub, Expr::Float(b)) => Some(Expr::Float(a - b)),
        (Expr::Float(a), BinOp::Mul, Expr::Float(b)) => Some(Expr::Float(a * b)),
        (Expr::Float(a), BinOp::Div, Expr::Float(b)) if *b != 0.0 => Some(Expr::Float(a / b)),

        // Mixed Int / Float promotion
        (Expr::Int(a), BinOp::Add, Expr::Float(b)) => Some(Expr::Float(*a as f64 + b)),
        (Expr::Float(a), BinOp::Add, Expr::Int(b)) => Some(Expr::Float(a + *b as f64)),
        (Expr::Int(a), BinOp::Mul, Expr::Float(b)) => Some(Expr::Float(*a as f64 * b)),
        (Expr::Float(a), BinOp::Mul, Expr::Int(b)) => Some(Expr::Float(a * *b as f64)),

        // Length arithmetic
        (Expr::Length(a, u1), BinOp::Add, Expr::Length(b, u2)) if u1 == u2 => {
            Some(Expr::Length(a + b, *u1))
        }
        (Expr::Length(a, u1), BinOp::Sub, Expr::Length(b, u2)) if u1 == u2 => {
            Some(Expr::Length(a - b, *u1))
        }

        // String concatenation
        (Expr::String(a), BinOp::Add, Expr::String(b)) => Some(Expr::String(format!("{a}{b}"))),

        // Boolean operations
        (Expr::Bool(true), BinOp::And, other) | (other, BinOp::And, Expr::Bool(true)) => {
            Some(other.clone())
        }
        (Expr::Bool(false), BinOp::And, _) | (_, BinOp::And, Expr::Bool(false)) => {
            Some(Expr::Bool(false))
        }
        (Expr::Bool(true), BinOp::Or, _) | (_, BinOp::Or, Expr::Bool(true)) => {
            Some(Expr::Bool(true))
        }
        (Expr::Bool(false), BinOp::Or, other) | (other, BinOp::Or, Expr::Bool(false)) => {
            Some(other.clone())
        }

        // Literal equality
        (Expr::Int(a), BinOp::Eq, Expr::Int(b)) => Some(Expr::Bool(a == b)),
        (Expr::Int(a), BinOp::Ne, Expr::Int(b)) => Some(Expr::Bool(a != b)),
        (Expr::Int(a), BinOp::Lt, Expr::Int(b)) => Some(Expr::Bool(a < b)),
        (Expr::Int(a), BinOp::Le, Expr::Int(b)) => Some(Expr::Bool(a <= b)),
        (Expr::Int(a), BinOp::Gt, Expr::Int(b)) => Some(Expr::Bool(a > b)),
        (Expr::Int(a), BinOp::Ge, Expr::Int(b)) => Some(Expr::Bool(a >= b)),

        (Expr::Float(a), BinOp::Eq, Expr::Float(b)) => Some(Expr::Bool(a == b)),
        (Expr::Float(a), BinOp::Ne, Expr::Float(b)) => Some(Expr::Bool(a != b)),
        (Expr::Float(a), BinOp::Lt, Expr::Float(b)) => Some(Expr::Bool(a < b)),
        (Expr::Float(a), BinOp::Le, Expr::Float(b)) => Some(Expr::Bool(a <= b)),
        (Expr::Float(a), BinOp::Gt, Expr::Float(b)) => Some(Expr::Bool(a > b)),
        (Expr::Float(a), BinOp::Ge, Expr::Float(b)) => Some(Expr::Bool(a >= b)),

        (Expr::String(a), BinOp::Eq, Expr::String(b)) => Some(Expr::Bool(a == b)),
        (Expr::String(a), BinOp::Ne, Expr::String(b)) => Some(Expr::Bool(a != b)),

        _ => None,
    }
}

fn get_num(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Float(f) => Some(*f),
        Expr::Int(i) => Some(*i as f64),
        Expr::Length(v, _) => Some(*v as f64),
        _ => None,
    }
}

fn fold_math_call(name: &str, args: &[Expr]) -> Option<Expr> {
    match name {
        "sin" if args.len() == 1 => get_num(&args[0]).map(|v| Expr::Float(v.sin())),
        "cos" if args.len() == 1 => get_num(&args[0]).map(|v| Expr::Float(v.cos())),
        "tan" if args.len() == 1 => get_num(&args[0]).map(|v| Expr::Float(v.tan())),
        "sqrt" if args.len() == 1 => get_num(&args[0]).map(|v| Expr::Float(v.sqrt())),
        "abs" if args.len() == 1 => match &args[0] {
            Expr::Int(i) => Some(Expr::Int(i.abs())),
            other => get_num(other).map(|v| Expr::Float(v.abs())),
        },
        "round" if args.len() == 1 => get_num(&args[0]).map(|v| Expr::Float(v.round())),
        "floor" if args.len() == 1 => get_num(&args[0]).map(|v| Expr::Float(v.floor())),
        "ceil" if args.len() == 1 => get_num(&args[0]).map(|v| Expr::Float(v.ceil())),
        "min" if args.len() == 2 => match (&args[0], &args[1]) {
            (Expr::Int(a), Expr::Int(b)) => Some(Expr::Int((*a).min(*b))),
            _ => {
                let a = get_num(&args[0])?;
                let b = get_num(&args[1])?;
                Some(Expr::Float(a.min(b)))
            }
        },
        "max" if args.len() == 2 => match (&args[0], &args[1]) {
            (Expr::Int(a), Expr::Int(b)) => Some(Expr::Int((*a).max(*b))),
            _ => {
                let a = get_num(&args[0])?;
                let b = get_num(&args[1])?;
                Some(Expr::Float(a.max(b)))
            }
        },
        "pow" if args.len() == 2 => {
            let base = get_num(&args[0])?;
            let exp = get_num(&args[1])?;
            Some(Expr::Float(base.powf(exp)))
        }
        "clamp" if args.len() == 3 => match (&args[0], &args[1], &args[2]) {
            (Expr::Int(v), Expr::Int(lo), Expr::Int(hi)) => {
                let hi_c = (*hi).max(*lo);
                Some(Expr::Int((*v).clamp(*lo, hi_c)))
            }
            _ => {
                let v = get_num(&args[0])?;
                let lo = get_num(&args[1])?;
                let hi = get_num(&args[2])?;
                let hi_c = hi.max(lo);
                Some(Expr::Float(v.clamp(lo, hi_c)))
            }
        },
        _ => None,
    }
}
