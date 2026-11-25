use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub span: Span,
    pub severity: Severity,
    pub message: String,
}

/// Property / field / callback parameter type expression supporting generics & lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeExpr {
    Named(String),
    List(Box<TypeExpr>),
    Generic(String, Vec<TypeExpr>),
}

impl TypeExpr {
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into())
    }

    pub fn base_name(&self) -> Option<&str> {
        match self {
            Self::Named(n) | Self::Generic(n, _) => Some(n.as_str()),
            Self::List(_) => None,
        }
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => write!(f, "{name}"),
            Self::List(inner) => write!(f, "[{inner}]"),
            Self::Generic(name, args) => {
                write!(f, "{name}<")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ">")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructField {
    pub name: String,
    pub ty: TypeExpr,
    pub default_value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDecl {
    pub exported: bool,
    pub name: String,
    pub fields: Vec<StructField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumDecl {
    pub exported: bool,
    pub name: String,
    pub variants: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuiFile {
    pub imports: Vec<ImportDecl>,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub components: Vec<ComponentDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportDecl {
    pub symbols: Vec<String>,
    pub from_path: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyDir {
    In,
    Out,
    InOut,
    Private,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyDecl {
    pub dir: PropertyDir,
    pub ty: TypeExpr,
    pub name: String,
    pub default_value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallbackDecl {
    pub name: String,
    pub params: Vec<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentDecl {
    pub exported: bool,
    pub name: String,
    pub properties: Vec<PropertyDecl>,
    pub callbacks: Vec<CallbackDecl>,
    pub root: Option<ElementNode>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimateSpec {
    pub prop: String,
    pub fields: Vec<(String, Expr)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateDecl {
    pub name: String,
    pub condition: Expr,
    pub property_overrides: Vec<PropBinding>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransitionSpec {
    pub from_state: Option<String>,
    pub to_state: Option<String>,
    pub animations: Vec<AnimateSpec>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElementNode {
    pub kind: String,
    pub props: Vec<PropBinding>,
    pub handlers: Vec<HandlerBinding>,
    pub animations: Vec<AnimateSpec>,
    pub states: Vec<StateDecl>,
    pub transitions: Vec<TransitionSpec>,
    pub children: Vec<ChildItem>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropBinding {
    pub name: String,
    pub two_way: bool,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandlerBinding {
    pub event: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChildItem {
    Element(ElementNode),
    For {
        item_var: String,
        index_var: Option<String>,
        iterable: Expr,
        key: Option<Expr>,
        body: Box<ElementNode>,
        span: Span,
    },
    If {
        condition: Expr,
        then_branch: Box<ElementNode>,
        else_branch: Option<Box<ElementNode>>,
        span: Span,
    },
    ChildrenSlot {
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    Call {
        callee: String,
        args: Vec<Expr>,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    Dp,
    Px,
    Pct,
    Deg,
    Rad,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Ident(String),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    ColorHex(String),
    Length(f32, Unit),
    DurationSec(f32),
    Array(Vec<Expr>),
    Call(String, Vec<Expr>),
    MethodCall(Box<Expr>, String, Vec<Expr>),
    UnaryNeg(Box<Expr>),
    UnaryNot(Box<Expr>),
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Ternary {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
}
