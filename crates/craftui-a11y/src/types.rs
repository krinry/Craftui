use craftui_core::{NodeId, Rect};

/// Semantic UI element role mapped to `accesskit::Role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum A11yRole {
    Window,
    #[default]
    Container,
    Label,
    Button,
    TextInput,
    MultilineTextInput,
    CheckBox,
    RadioButton,
    Switch,
    Slider,
    ComboBox,
    TabList,
    Tab,
    Menu,
    MenuItem,
    Tooltip,
    Divider,
    ProgressIndicator,
    Image,
    ScrollView,
    Canvas,
    Dialog,
    Alert,
    List,
    ListItem,
}

impl A11yRole {
    pub fn to_accesskit(self) -> accesskit::Role {
        match self {
            Self::Window => accesskit::Role::Window,
            Self::Container => accesskit::Role::GenericContainer,
            Self::Label => accesskit::Role::Label,
            Self::Button => accesskit::Role::Button,
            Self::TextInput => accesskit::Role::TextInput,
            Self::MultilineTextInput => accesskit::Role::MultilineTextInput,
            Self::CheckBox => accesskit::Role::CheckBox,
            Self::RadioButton => accesskit::Role::RadioButton,
            Self::Switch => accesskit::Role::Switch,
            Self::Slider => accesskit::Role::Slider,
            Self::ComboBox => accesskit::Role::ComboBox,
            Self::TabList => accesskit::Role::TabList,
            Self::Tab => accesskit::Role::Tab,
            Self::Menu => accesskit::Role::Menu,
            Self::MenuItem => accesskit::Role::MenuItem,
            Self::Tooltip => accesskit::Role::Tooltip,
            Self::Divider => accesskit::Role::Splitter,
            Self::ProgressIndicator => accesskit::Role::ProgressIndicator,
            Self::Image => accesskit::Role::Image,
            Self::ScrollView => accesskit::Role::ScrollView,
            Self::Canvas => accesskit::Role::Canvas,
            Self::Dialog => accesskit::Role::Dialog,
            Self::Alert => accesskit::Role::Alert,
            Self::List => accesskit::Role::List,
            Self::ListItem => accesskit::Role::ListItem,
        }
    }
}

/// Accessibility properties attached to a widget node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct A11yMetadata {
    pub role: A11yRole,
    pub label: Option<String>,
    pub value: Option<String>,
    pub numeric_value: Option<f64>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub focusable: bool,
}

/// Snapshot node handed to [`A11yBridge`] after layout.
#[derive(Debug, Clone)]
pub struct A11ySnapshotNode {
    pub id: NodeId,
    pub bounds: Rect,
    pub meta: A11yMetadata,
    pub children: Vec<NodeId>,
}

/// High-level action decoded from `accesskit::ActionRequest`.
#[derive(Debug, Clone, PartialEq)]
pub enum A11yAction {
    Click(NodeId),
    Focus(NodeId),
    SetValue(NodeId, String),
    SetNumericValue(NodeId, f64),
    Increment(NodeId),
    Decrement(NodeId),
    ScrollIntoView(NodeId),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CachedA11yNode {
    pub is_root: bool,
    pub bounds: Rect,
    pub meta: A11yMetadata,
    pub children: Vec<NodeId>,
    pub window_title: String,
}
