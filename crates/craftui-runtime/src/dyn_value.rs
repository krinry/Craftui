use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::rc::Rc;

use craftui_core::{Brush, Color, Length, TextureId};
use craftui_reactive::{Scope, Signal};

#[derive(Debug, Clone, PartialEq)]
pub enum DynValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Color(Color),
    Length(Length),
    Texture(Option<TextureId>),
    Brush(Brush),
    List(Vec<DynValue>),
    Struct {
        name: String,
        fields: BTreeMap<String, DynValue>,
    },
    Enum(String, String),
    Null,
}

impl DynValue {
    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Float(v) => *v as f32,
            Self::Int(v) => *v as f32,
            Self::Length(Length::Dp(v) | Length::Px(v) | Length::Pct(v)) => *v,
            _ => 0.0,
        }
    }

    pub fn as_i64(&self) -> i64 {
        match self {
            Self::Int(v) => *v,
            Self::Float(v) => *v as i64,
            Self::Bool(b) => i64::from(*b),
            _ => 0,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::List(v) => !v.is_empty(),
            _ => false,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => {
                if (f.fract()).abs() < 1e-6 {
                    format!("{f:.1}")
                } else {
                    format!("{f}")
                }
            }
            Self::Bool(b) => b.to_string(),
            Self::Color(c) => c.to_hex(),
            Self::Enum(_, v) => v.clone(),
            Self::Struct { name, fields } => {
                let mut out = format!("{name} {{ ");
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(out, "{k}: {}", v.as_str());
                }
                out.push_str(" }");
                out
            }
            Self::List(items) => {
                let joined: Vec<String> = items.iter().map(DynValue::as_str).collect();
                format!("[{}]", joined.join(", "))
            }
            Self::Length(Length::Dp(v)) => format!("{v}dp"),
            Self::Length(Length::Px(v)) => format!("{v}px"),
            Self::Length(Length::Pct(v)) => format!("{v}%"),
            Self::Length(Length::Auto) => "auto".to_string(),
            Self::Texture(Some(id)) => format!("{id:?}"),
            Self::Texture(None) | Self::Null => String::new(),
            Self::Brush(_) => "brush".to_string(),
        }
    }

    pub fn as_length(&self) -> Length {
        match self {
            Self::Length(l) => *l,
            Self::Float(v) => Length::Dp(*v as f32),
            Self::Int(v) => Length::Dp(*v as f32),
            _ => Length::Auto,
        }
    }

    pub fn as_brush(&self) -> Option<Brush> {
        match self {
            Self::Brush(b) => Some(b.clone()),
            Self::Color(c) => Some(Brush::Solid(*c)),
            Self::String(s) => Color::from_hex(s).map(Brush::Solid),
            _ => None,
        }
    }

    pub fn as_color(&self) -> Option<Color> {
        match self {
            Self::Color(c) => Some(*c),
            Self::Brush(Brush::Solid(c)) => Some(*c),
            Self::String(s) => Color::from_hex(s),
            _ => None,
        }
    }

    pub fn as_string_list(&self) -> Vec<String> {
        match self {
            Self::List(items) => items.iter().map(DynValue::as_str).collect(),
            _ => Vec::new(),
        }
    }

    pub fn as_string_table(&self) -> Vec<Vec<String>> {
        match self {
            Self::List(rows) => rows
                .iter()
                .map(|row| match row {
                    Self::List(cells) => cells.iter().map(DynValue::as_str).collect(),
                    other => vec![other.as_str()],
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn key_string(&self) -> String {
        match self {
            Self::Int(i) => format!("i:{i}"),
            Self::Float(f) => format!("f:{f:.4}"),
            Self::Bool(b) => format!("b:{b}"),
            Self::String(s) => format!("s:{s}"),
            Self::Color(c) => format!("c:{}", c.to_hex()),
            Self::Enum(n, v) => format!("e:{n}.{v}"),
            Self::Struct { name, fields } => {
                let mut s = format!("st:{name}{{");
                for (k, v) in fields {
                    let _ = write!(s, "{k}:{},", v.key_string());
                }
                s.push('}');
                s
            }
            Self::List(items) => {
                let parts: Vec<String> = items.iter().map(DynValue::key_string).collect();
                format!("l:[{}]", parts.join(","))
            }
            Self::Texture(id) => format!("t:{id:?}"),
            Self::Length(l) => format!("len:{l:?}"),
            Self::Brush(_) => "brush".to_string(),
            Self::Null => "null".to_string(),
        }
    }
}

pub type DynCallback = Rc<RefCell<dyn FnMut(Vec<DynValue>)>>;

/// Live state store preserved across `.cui` hot-reloads.
pub struct DynamicStore {
    pub props: HashMap<String, Signal<DynValue>>,
    pub callbacks: HashMap<String, DynCallback>,
}

impl Default for DynamicStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicStore {
    pub fn new() -> Self {
        Self {
            props: HashMap::new(),
            callbacks: HashMap::new(),
        }
    }

    pub fn get_or_create_prop(
        &mut self,
        scope: Scope,
        name: &str,
        initial: DynValue,
    ) -> Signal<DynValue> {
        if let Some(&sig) = self.props.get(name)
            && sig.is_alive()
        {
            return sig;
        }
        let sig = scope.signal(initial);
        self.props.insert(name.to_string(), sig);
        sig
    }

    pub fn set_prop(&mut self, scope: Scope, name: &str, value: DynValue) {
        let sig = self.get_or_create_prop(scope, name, value.clone());
        sig.set(value);
    }

    pub fn register_callback(
        &mut self,
        name: impl Into<String>,
        cb: impl FnMut(Vec<DynValue>) + 'static,
    ) {
        self.callbacks
            .insert(name.into(), Rc::new(RefCell::new(cb)));
    }
}
