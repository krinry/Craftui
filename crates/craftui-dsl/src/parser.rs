use crate::ast::*;
use crate::cst::{SyntaxKind, SyntaxNode};
use crate::lexer::{Token, TokenKind, tokenize};
use rowan::GreenNodeBuilder;

pub struct ParseResult {
    pub file: CuiFile,
    pub cst: SyntaxNode,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(source: &str) -> ParseResult {
    let tokens = tokenize(source);
    let mut parser = Parser {
        tokens,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        diagnostics: Vec::new(),
    };
    let file = parser.parse_file();
    let green = parser.builder.finish();
    let cst = SyntaxNode::new_root(green);
    ParseResult {
        file,
        cst,
        diagnostics: parser.diagnostics,
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn eat_trivia(&mut self) {
        while let Some(tok) = self.tokens.get(self.pos) {
            if tok.kind.is_trivia() {
                self.builder
                    .token(SyntaxKind::from(tok.kind).into(), &tok.text);
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<TokenKind> {
        let mut idx = self.pos;
        while let Some(tok) = self.tokens.get(idx) {
            if !tok.kind.is_trivia() {
                return Some(tok.kind);
            }
            idx += 1;
        }
        None
    }

    fn peek_nth(&self, n: usize) -> Option<TokenKind> {
        let mut idx = self.pos;
        let mut seen = 0usize;
        while let Some(tok) = self.tokens.get(idx) {
            if !tok.kind.is_trivia() {
                if seen == n {
                    return Some(tok.kind);
                }
                seen += 1;
            }
            idx += 1;
        }
        None
    }

    fn peek_text(&self) -> Option<&str> {
        let mut idx = self.pos;
        while let Some(tok) = self.tokens.get(idx) {
            if !tok.kind.is_trivia() {
                return Some(tok.text.as_str());
            }
            idx += 1;
        }
        None
    }

    fn current_offset(&self) -> usize {
        let mut idx = self.pos;
        while let Some(tok) = self.tokens.get(idx) {
            if !tok.kind.is_trivia() {
                return tok.start;
            }
            idx += 1;
        }
        self.tokens.last().map(|t| t.end).unwrap_or(0)
    }

    fn bump(&mut self) -> Option<Token> {
        self.eat_trivia();
        let tok = self.tokens.get(self.pos)?.clone();
        self.builder
            .token(SyntaxKind::from(tok.kind).into(), &tok.text);
        self.pos += 1;
        Some(tok)
    }

    fn expect(&mut self, expected: TokenKind, msg: &str) -> Option<Token> {
        if self.peek() == Some(expected) {
            self.bump()
        } else {
            let off = self.current_offset();
            self.diagnostics.push(Diagnostic {
                span: Span::new(off, off + 1),
                severity: Severity::Error,
                message: msg.to_string(),
            });
            None
        }
    }

    fn eat_if(&mut self, kind: TokenKind) -> bool {
        if self.peek() == Some(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn parse_file(&mut self) -> CuiFile {
        self.builder.start_node(SyntaxKind::Root.into());
        let mut imports = Vec::new();
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        let mut components = Vec::new();

        while let Some(kind) = self.peek() {
            match kind {
                TokenKind::KwImport => imports.push(self.parse_import()),
                TokenKind::KwStruct => structs.push(self.parse_struct()),
                TokenKind::KwEnum => enums.push(self.parse_enum()),
                TokenKind::KwComponent => components.push(self.parse_component()),
                TokenKind::KwExport => match self.peek_nth(1) {
                    Some(TokenKind::KwStruct) => structs.push(self.parse_struct()),
                    Some(TokenKind::KwEnum) => enums.push(self.parse_enum()),
                    _ => components.push(self.parse_component()),
                },
                _ => {
                    self.builder.start_node(SyntaxKind::ErrorNode.into());
                    if let Some(tok) = self.bump() {
                        self.diagnostics.push(Diagnostic {
                            span: Span::new(tok.start, tok.end),
                            severity: Severity::Error,
                            message: format!("unexpected top-level token '{}'", tok.text),
                        });
                    }
                    self.builder.finish_node();
                }
            }
        }
        self.eat_trivia();
        self.builder.finish_node();
        CuiFile {
            imports,
            structs,
            enums,
            components,
        }
    }

    fn parse_import(&mut self) -> ImportDecl {
        self.builder.start_node(SyntaxKind::ImportDecl.into());
        let start = self.current_offset();
        self.bump(); // import
        let mut symbols = Vec::new();
        if self.eat_if(TokenKind::LBrace) {
            while let Some(pk) = self.peek() {
                if pk == TokenKind::RBrace {
                    break;
                }
                if pk == TokenKind::Ident
                    && let Some(t) = self.bump()
                {
                    symbols.push(t.text);
                } else if pk == TokenKind::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RBrace, "expected '}' in import list");
        }
        self.expect(TokenKind::KwFrom, "expected 'from' in import statement");
        let from_path = if self.peek() == Some(TokenKind::StringLit) {
            let t = self.bump().unwrap();
            unquote(&t.text)
        } else {
            self.expect(TokenKind::StringLit, "expected quoted module path");
            String::new()
        };
        self.eat_if(TokenKind::Semi);
        let end = self.current_offset();
        self.builder.finish_node();
        ImportDecl {
            symbols,
            from_path,
            span: Span::new(start, end),
        }
    }

    fn parse_struct(&mut self) -> StructDecl {
        self.builder.start_node(SyntaxKind::StructDecl.into());
        let start = self.current_offset();
        let exported = self.eat_if(TokenKind::KwExport);
        self.expect(TokenKind::KwStruct, "expected 'struct'");
        let name = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected struct name");
            "UnnamedStruct".into()
        };
        self.expect(TokenKind::LBrace, "expected '{' after struct name");
        let mut fields = Vec::new();
        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBrace {
                break;
            }
            if pk == TokenKind::Comma || pk == TokenKind::Semi {
                self.bump();
                continue;
            }
            let f_start = self.current_offset();
            let fname = if pk == TokenKind::Ident {
                self.bump().unwrap().text
            } else {
                self.expect(TokenKind::Ident, "expected struct field name");
                break;
            };
            self.expect(TokenKind::Colon, "expected ':' after struct field name");
            let ty = self.parse_type_expr();
            let default_value = if self.eat_if(TokenKind::Eq) {
                Some(self.parse_expr())
            } else {
                None
            };
            let _ = self.eat_if(TokenKind::Comma) || self.eat_if(TokenKind::Semi);
            let f_end = self.current_offset();
            fields.push(StructField {
                name: fname,
                ty,
                default_value,
                span: Span::new(f_start, f_end),
            });
        }
        self.expect(TokenKind::RBrace, "expected '}' closing struct");
        let end = self.current_offset();
        self.builder.finish_node();
        StructDecl {
            exported,
            name,
            fields,
            span: Span::new(start, end),
        }
    }

    fn parse_enum(&mut self) -> EnumDecl {
        self.builder.start_node(SyntaxKind::EnumDecl.into());
        let start = self.current_offset();
        let exported = self.eat_if(TokenKind::KwExport);
        self.expect(TokenKind::KwEnum, "expected 'enum'");
        let name = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected enum name");
            "UnnamedEnum".into()
        };
        self.expect(TokenKind::LBrace, "expected '{' after enum name");
        let mut variants = Vec::new();
        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBrace {
                break;
            }
            if pk == TokenKind::Ident {
                variants.push(self.bump().unwrap().text);
                let _ = self.eat_if(TokenKind::Comma) || self.eat_if(TokenKind::Semi);
            } else if pk == TokenKind::Comma || pk == TokenKind::Semi {
                self.bump();
            } else {
                self.diagnostics.push(Diagnostic {
                    span: Span::new(self.current_offset(), self.current_offset() + 1),
                    severity: Severity::Error,
                    message: "expected enum variant identifier".into(),
                });
                self.bump();
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' closing enum");
        let end = self.current_offset();
        self.builder.finish_node();
        EnumDecl {
            exported,
            name,
            variants,
            span: Span::new(start, end),
        }
    }

    fn parse_component(&mut self) -> ComponentDecl {
        self.builder.start_node(SyntaxKind::ComponentDecl.into());
        let start = self.current_offset();
        let exported = self.eat_if(TokenKind::KwExport);
        self.expect(TokenKind::KwComponent, "expected 'component'");
        let name = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected component name");
            "Unnamed".into()
        };
        self.expect(TokenKind::LBrace, "expected '{' after component name");

        let mut properties = Vec::new();
        let mut callbacks = Vec::new();
        let mut root = None;

        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBrace {
                break;
            }
            match pk {
                TokenKind::KwIn | TokenKind::KwOut | TokenKind::KwInOut | TokenKind::KwProperty => {
                    properties.push(self.parse_property_decl());
                }
                TokenKind::KwCallback => {
                    callbacks.push(self.parse_callback_decl());
                }
                TokenKind::Ident => {
                    root = Some(self.parse_element());
                }
                _ => {
                    self.builder.start_node(SyntaxKind::ErrorNode.into());
                    if let Some(t) = self.bump() {
                        self.diagnostics.push(Diagnostic {
                            span: Span::new(t.start, t.end),
                            severity: Severity::Error,
                            message: format!("unexpected token '{}' in component body", t.text),
                        });
                    }
                    self.builder.finish_node();
                }
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' closing component");
        let end = self.current_offset();
        self.builder.finish_node();
        ComponentDecl {
            exported,
            name,
            properties,
            callbacks,
            root,
            span: Span::new(start, end),
        }
    }

    fn parse_type_expr(&mut self) -> TypeExpr {
        self.builder.start_node(SyntaxKind::TypeExprNode.into());
        let ty = match self.peek() {
            Some(TokenKind::LBracket) => {
                self.bump(); // [
                let inner = self.parse_type_expr();
                self.expect(TokenKind::RBracket, "expected ']' closing list type");
                TypeExpr::List(Box::new(inner))
            }
            Some(TokenKind::Ident) => {
                let name = self.bump().unwrap().text;
                if self.eat_if(TokenKind::Lt) {
                    let mut args = Vec::new();
                    while let Some(pk) = self.peek() {
                        if pk == TokenKind::Gt {
                            break;
                        }
                        args.push(self.parse_type_expr());
                        if !self.eat_if(TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::Gt, "expected '>' closing generic type arguments");
                    TypeExpr::Generic(name, args)
                } else {
                    TypeExpr::Named(name)
                }
            }
            _ => {
                let off = self.current_offset();
                self.diagnostics.push(Diagnostic {
                    span: Span::new(off, off + 1),
                    severity: Severity::Error,
                    message: "expected type name".into(),
                });
                TypeExpr::Named("unknown".into())
            }
        };
        self.builder.finish_node();
        ty
    }

    fn parse_property_decl(&mut self) -> PropertyDecl {
        self.builder.start_node(SyntaxKind::PropertyDecl.into());
        let start = self.current_offset();
        let dir = match self.peek() {
            Some(TokenKind::KwIn) => {
                self.bump();
                PropertyDir::In
            }
            Some(TokenKind::KwOut) => {
                self.bump();
                PropertyDir::Out
            }
            Some(TokenKind::KwInOut) => {
                self.bump();
                PropertyDir::InOut
            }
            _ => PropertyDir::Private,
        };
        self.expect(TokenKind::KwProperty, "expected 'property'");
        self.expect(TokenKind::Lt, "expected '<' before property type");
        let ty = self.parse_type_expr();
        self.expect(TokenKind::Gt, "expected '>' after property type");
        let name = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected property name");
            "prop".into()
        };
        let default_value = if self.eat_if(TokenKind::Colon) {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(TokenKind::Semi, "expected ';' after property declaration");
        let end = self.current_offset();
        self.builder.finish_node();
        PropertyDecl {
            dir,
            ty,
            name,
            default_value,
            span: Span::new(start, end),
        }
    }

    fn parse_callback_decl(&mut self) -> CallbackDecl {
        self.builder.start_node(SyntaxKind::CallbackDecl.into());
        let start = self.current_offset();
        self.bump(); // callback
        let name = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected callback name");
            "cb".into()
        };
        let mut params = Vec::new();
        if self.eat_if(TokenKind::LParen) {
            while let Some(pk) = self.peek() {
                if pk == TokenKind::RParen {
                    break;
                }
                params.push(self.parse_type_expr());
                if !self.eat_if(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RParen, "expected ')' after callback params");
        }
        self.expect(TokenKind::Semi, "expected ';' after callback declaration");
        let end = self.current_offset();
        self.builder.finish_node();
        CallbackDecl {
            name,
            params,
            span: Span::new(start, end),
        }
    }

    fn parse_element(&mut self) -> ElementNode {
        self.builder.start_node(SyntaxKind::Element.into());
        let start = self.current_offset();
        let kind = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected element type name");
            "View".into()
        };
        self.expect(TokenKind::LBrace, "expected '{' after element name");

        let mut props = Vec::new();
        let mut handlers = Vec::new();
        let mut animations = Vec::new();
        let mut states = Vec::new();
        let mut transitions = Vec::new();
        let mut children = Vec::new();

        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBrace {
                break;
            }
            match pk {
                TokenKind::KwFor => {
                    children.push(self.parse_for());
                }
                TokenKind::KwIf => {
                    children.push(self.parse_if());
                }
                TokenKind::AtChildren => {
                    self.builder.start_node(SyntaxKind::ChildrenSlot.into());
                    let tok = self.bump().expect("AtChildren token");
                    if self.peek() == Some(TokenKind::Semi) {
                        self.bump();
                    }
                    self.builder.finish_node();
                    children.push(ChildItem::ChildrenSlot {
                        span: Span::new(tok.start, tok.end),
                    });
                }
                TokenKind::Ident => {
                    if self.peek_text() == Some("animate")
                        && self.peek_nth(1) == Some(TokenKind::Ident)
                        && matches!(self.peek_nth(2), Some(TokenKind::Comma | TokenKind::LBrace))
                    {
                        animations.extend(self.parse_animate_block());
                        continue;
                    }
                    if self.peek_text() == Some("states")
                        && self.peek_nth(1) == Some(TokenKind::LBracket)
                    {
                        states.extend(self.parse_states_block());
                        continue;
                    }
                    if self.peek_text() == Some("transitions")
                        && self.peek_nth(1) == Some(TokenKind::LBracket)
                    {
                        transitions.extend(self.parse_transitions_block());
                        continue;
                    }
                    match self.peek_nth(1) {
                        Some(TokenKind::LBrace) => {
                            children.push(ChildItem::Element(self.parse_element()));
                        }
                        Some(TokenKind::Colon) => {
                            props.push(self.parse_prop_binding(false));
                        }
                        Some(TokenKind::TwoWayBind) => {
                            props.push(self.parse_prop_binding(true));
                        }
                        Some(TokenKind::FatArrow) => {
                            handlers.push(self.parse_handler_binding());
                        }
                        _ => {
                            self.builder.start_node(SyntaxKind::ErrorNode.into());
                            if let Some(t) = self.bump() {
                                self.diagnostics.push(Diagnostic {
                                    span: Span::new(t.start, t.end),
                                    severity: Severity::Error,
                                    message: format!(
                                        "unexpected identifier '{}' in element",
                                        t.text
                                    ),
                                });
                            }
                            self.builder.finish_node();
                        }
                    }
                }
                _ => {
                    self.builder.start_node(SyntaxKind::ErrorNode.into());
                    if let Some(t) = self.bump() {
                        self.diagnostics.push(Diagnostic {
                            span: Span::new(t.start, t.end),
                            severity: Severity::Error,
                            message: format!("unexpected token '{}' in element body", t.text),
                        });
                    }
                    self.builder.finish_node();
                }
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' closing element");
        let end = self.current_offset();
        self.builder.finish_node();
        ElementNode {
            kind,
            props,
            handlers,
            animations,
            states,
            transitions,
            children,
            span: Span::new(start, end),
        }
    }

    fn parse_states_block(&mut self) -> Vec<StateDecl> {
        self.builder.start_node(SyntaxKind::StatesBlock.into());
        let _ = self.bump(); // 'states'
        self.expect(TokenKind::LBracket, "expected '[' after 'states'");
        let mut result = Vec::new();
        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBracket {
                break;
            }
            if pk == TokenKind::Ident {
                let start = self.current_offset();
                let name = self.bump().unwrap().text;
                if self.peek_text() == Some("when") {
                    self.bump();
                }
                let condition = self.parse_expr();
                self.eat_if(TokenKind::Colon);
                self.expect(TokenKind::LBrace, "expected '{' after state condition");
                let mut property_overrides = Vec::new();
                while let Some(inner) = self.peek() {
                    if inner == TokenKind::RBrace {
                        break;
                    }
                    if inner == TokenKind::Ident {
                        property_overrides.push(self.parse_prop_binding(false));
                    } else {
                        let _ = self.bump();
                    }
                }
                self.expect(TokenKind::RBrace, "expected '}' closing state declaration");
                let end = self.current_offset();
                result.push(StateDecl {
                    name,
                    condition,
                    property_overrides,
                    span: Span::new(start, end),
                });
            } else {
                let _ = self.bump();
            }
        }
        self.expect(TokenKind::RBracket, "expected ']' closing states block");
        self.builder.finish_node();
        result
    }

    fn parse_transitions_block(&mut self) -> Vec<TransitionSpec> {
        self.builder.start_node(SyntaxKind::TransitionsBlock.into());
        let _ = self.bump(); // 'transitions'
        self.expect(TokenKind::LBracket, "expected '[' after 'transitions'");
        let mut result = Vec::new();
        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBracket {
                break;
            }
            let start = self.current_offset();
            let mut from_state = None;
            let mut to_state = None;
            if self.peek_text() == Some("in") {
                self.bump();
                if self.peek() == Some(TokenKind::Ident) {
                    to_state = Some(self.bump().unwrap().text);
                }
            } else if self.peek_text() == Some("out") {
                self.bump();
                if self.peek() == Some(TokenKind::Ident) {
                    from_state = Some(self.bump().unwrap().text);
                }
            } else if self.peek_text() == Some("from") {
                self.bump();
                if self.peek() == Some(TokenKind::Ident) {
                    from_state = Some(self.bump().unwrap().text);
                }
                if self.peek_text() == Some("to") {
                    self.bump();
                    if self.peek() == Some(TokenKind::Ident) {
                        to_state = Some(self.bump().unwrap().text);
                    }
                }
            }
            self.eat_if(TokenKind::Colon);
            self.expect(TokenKind::LBrace, "expected '{' opening transition block");
            let mut animations = Vec::new();
            while let Some(inner) = self.peek() {
                if inner == TokenKind::RBrace {
                    break;
                }
                if self.peek_text() == Some("animate") {
                    animations.extend(self.parse_animate_block());
                } else {
                    let _ = self.bump();
                }
            }
            self.expect(TokenKind::RBrace, "expected '}' closing transition block");
            let end = self.current_offset();
            result.push(TransitionSpec {
                from_state,
                to_state,
                animations,
                span: Span::new(start, end),
            });
        }
        self.expect(
            TokenKind::RBracket,
            "expected ']' closing transitions block",
        );
        self.builder.finish_node();
        result
    }

    fn parse_animate_block(&mut self) -> Vec<AnimateSpec> {
        self.builder.start_node(SyntaxKind::AnimateBlock.into());
        let start = self.current_offset();
        self.bump(); // 'animate'
        let mut target_props = Vec::new();
        while let Some(pk) = self.peek() {
            if pk == TokenKind::LBrace {
                break;
            }
            if pk == TokenKind::Ident {
                target_props.push(self.bump().unwrap().text);
                let _ = self.eat_if(TokenKind::Comma);
            } else if pk == TokenKind::Comma {
                self.bump();
            } else {
                let off = self.current_offset();
                self.diagnostics.push(Diagnostic {
                    span: Span::new(off, off + 1),
                    severity: Severity::Error,
                    message: "expected animated property name or '{' in animate block".into(),
                });
                break;
            }
        }
        self.expect(TokenKind::LBrace, "expected '{' in animate block");
        let mut fields = Vec::new();
        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBrace {
                break;
            }
            if pk == TokenKind::Semi || pk == TokenKind::Comma {
                self.bump();
                continue;
            }
            if pk == TokenKind::Ident {
                let fname = self.bump().unwrap().text;
                if self.eat_if(TokenKind::Colon) {
                    let val = self.parse_expr();
                    fields.push((fname, val));
                    let _ = self.eat_if(TokenKind::Semi) || self.eat_if(TokenKind::Comma);
                } else if self.eat_if(TokenKind::LParen) {
                    let args = self.parse_call_args();
                    fields.push(("curve".to_string(), Expr::Call(fname, args)));
                    let _ = self.eat_if(TokenKind::Semi) || self.eat_if(TokenKind::Comma);
                } else {
                    self.expect(TokenKind::Colon, "expected ':' or '(' in animate field");
                    break;
                }
            } else {
                self.builder.start_node(SyntaxKind::ErrorNode.into());
                if let Some(t) = self.bump() {
                    self.diagnostics.push(Diagnostic {
                        span: Span::new(t.start, t.end),
                        severity: Severity::Error,
                        message: format!("unexpected token '{}' in animate block", t.text),
                    });
                }
                self.builder.finish_node();
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' closing animate block");
        let end = self.current_offset();
        self.builder.finish_node();
        target_props
            .into_iter()
            .map(|prop| AnimateSpec {
                prop,
                fields: fields.clone(),
                span: Span::new(start, end),
            })
            .collect()
    }

    fn parse_prop_binding(&mut self, two_way: bool) -> PropBinding {
        let node_kind = if two_way {
            SyntaxKind::TwoWayAssign
        } else {
            SyntaxKind::PropertyAssign
        };
        self.builder.start_node(node_kind.into());
        let start = self.current_offset();
        let name = self.bump().map(|t| t.text).unwrap_or_default();
        self.bump(); // ':' or '<=>'
        let value = self.parse_expr();
        self.expect(TokenKind::Semi, "expected ';' after property binding");
        let end = self.current_offset();
        self.builder.finish_node();
        PropBinding {
            name,
            two_way,
            value,
            span: Span::new(start, end),
        }
    }

    fn parse_handler_binding(&mut self) -> HandlerBinding {
        self.builder.start_node(SyntaxKind::HandlerAssign.into());
        let start = self.current_offset();
        let event = self.bump().map(|t| t.text).unwrap_or_default();
        self.bump(); // '=>'
        let mut body = Vec::new();
        if self.eat_if(TokenKind::LBrace) {
            while let Some(pk) = self.peek() {
                if pk == TokenKind::RBrace {
                    break;
                }
                if pk == TokenKind::Semi {
                    self.bump();
                    continue;
                }
                body.push(self.parse_stmt());
                self.eat_if(TokenKind::Semi);
            }
            self.expect(TokenKind::RBrace, "expected '}' closing handler block");
        } else {
            body.push(self.parse_stmt());
            self.eat_if(TokenKind::Semi);
        }
        let end = self.current_offset();
        self.builder.finish_node();
        HandlerBinding {
            event,
            body,
            span: Span::new(start, end),
        }
    }

    fn parse_stmt(&mut self) -> Stmt {
        if self.peek() == Some(TokenKind::KwIf) {
            return self.parse_stmt_if();
        }
        let expr = self.parse_expr();
        let assign_op = match self.peek() {
            Some(TokenKind::Eq) => Some(AssignOp::Set),
            Some(TokenKind::PlusEq) => Some(AssignOp::Add),
            Some(TokenKind::MinusEq) => Some(AssignOp::Sub),
            Some(TokenKind::StarEq) => Some(AssignOp::Mul),
            Some(TokenKind::SlashEq) => Some(AssignOp::Div),
            _ => None,
        };
        if let Some(op) = assign_op {
            self.bump();
            let value = self.parse_expr();
            return Stmt::Assign {
                target: expr,
                op,
                value,
            };
        }
        if let Expr::Call(callee, args) = expr {
            Stmt::Call { callee, args }
        } else {
            Stmt::Expr(expr)
        }
    }

    fn parse_stmt_if(&mut self) -> Stmt {
        self.bump(); // 'if'
        let condition = self.parse_expr();
        let _ = self.eat_if(TokenKind::Colon);
        let then_branch = self.parse_stmt_block();
        let else_branch = if self.eat_if(TokenKind::KwElse) {
            if self.peek() == Some(TokenKind::KwIf) {
                vec![self.parse_stmt_if()]
            } else {
                let _ = self.eat_if(TokenKind::Colon);
                self.parse_stmt_block()
            }
        } else {
            Vec::new()
        };
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        }
    }

    fn parse_stmt_block(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        if self.eat_if(TokenKind::LBrace) {
            while let Some(pk) = self.peek() {
                if pk == TokenKind::RBrace {
                    break;
                }
                if pk == TokenKind::Semi {
                    self.bump();
                    continue;
                }
                stmts.push(self.parse_stmt());
                self.eat_if(TokenKind::Semi);
            }
            self.expect(TokenKind::RBrace, "expected '}' closing statement block");
        } else {
            stmts.push(self.parse_stmt());
            self.eat_if(TokenKind::Semi);
        }
        stmts
    }

    fn parse_for(&mut self) -> ChildItem {
        self.builder.start_node(SyntaxKind::ForLoop.into());
        let start = self.current_offset();
        self.bump(); // for
        let item_var = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected loop item identifier");
            "item".into()
        };
        let index_var = if self.eat_if(TokenKind::Comma) {
            if self.peek() == Some(TokenKind::Ident) {
                Some(self.bump().unwrap().text)
            } else {
                self.expect(TokenKind::Ident, "expected loop index identifier");
                None
            }
        } else {
            None
        };
        self.expect(TokenKind::KwIn, "expected 'in' in for loop");
        let iterable = self.parse_expr();
        let key = if self.peek_text() == Some("key") && self.peek_nth(1) == Some(TokenKind::Colon) {
            self.bump(); // key
            self.bump(); // :
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(TokenKind::Colon, "expected ':' before for-loop element");
        let body = Box::new(self.parse_element());
        let end = self.current_offset();
        self.builder.finish_node();
        ChildItem::For {
            item_var,
            index_var,
            iterable,
            key,
            body,
            span: Span::new(start, end),
        }
    }

    fn parse_if(&mut self) -> ChildItem {
        self.builder.start_node(SyntaxKind::IfCond.into());
        let start = self.current_offset();
        self.bump(); // if
        let condition = self.parse_expr();
        self.expect(TokenKind::Colon, "expected ':' after if condition");
        let then_branch = Box::new(self.parse_element());
        let else_branch = if self.eat_if(TokenKind::KwElse) {
            if self.peek() == Some(TokenKind::KwIf) {
                let nested_if = self.parse_if();
                Some(Box::new(ElementNode {
                    kind: "Container".into(),
                    props: Vec::new(),
                    handlers: Vec::new(),
                    animations: Vec::new(),
                    states: Vec::new(),
                    transitions: Vec::new(),
                    children: vec![nested_if],
                    span: Span::new(start, self.current_offset()),
                }))
            } else {
                self.eat_if(TokenKind::Colon);
                Some(Box::new(self.parse_element()))
            }
        } else {
            None
        };
        let end = self.current_offset();
        self.builder.finish_node();
        ChildItem::If {
            condition,
            then_branch,
            else_branch,
            span: Span::new(start, end),
        }
    }

    fn parse_expr(&mut self) -> Expr {
        self.builder.start_node(SyntaxKind::ExprNode.into());
        let expr = self.parse_ternary();
        self.builder.finish_node();
        expr
    }

    fn parse_ternary(&mut self) -> Expr {
        let cond = self.parse_range();
        if self.eat_if(TokenKind::Question) {
            let then_expr = self.parse_ternary();
            self.expect(TokenKind::Colon, "expected ':' in ternary expression");
            let else_expr = self.parse_ternary();
            Expr::Ternary {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            }
        } else {
            cond
        }
    }

    fn parse_range(&mut self) -> Expr {
        let start = self.parse_or();
        let inclusive = if self.eat_if(TokenKind::DotDotEq) {
            Some(true)
        } else if self.eat_if(TokenKind::DotDot) {
            Some(false)
        } else {
            None
        };
        if let Some(inclusive) = inclusive {
            let end = self.parse_or();
            Expr::Range {
                start: Box::new(start),
                end: Box::new(end),
                inclusive,
            }
        } else {
            start
        }
    }

    fn parse_or(&mut self) -> Expr {
        let mut lhs = self.parse_and();
        while self.eat_if(TokenKind::OrOr) {
            let rhs = self.parse_and();
            lhs = Expr::Binary(Box::new(lhs), BinOp::Or, Box::new(rhs));
        }
        lhs
    }

    fn parse_and(&mut self) -> Expr {
        let mut lhs = self.parse_eq();
        while self.eat_if(TokenKind::AndAnd) {
            let rhs = self.parse_eq();
            lhs = Expr::Binary(Box::new(lhs), BinOp::And, Box::new(rhs));
        }
        lhs
    }

    fn parse_eq(&mut self) -> Expr {
        let mut lhs = self.parse_cmp();
        loop {
            let op = match self.peek() {
                Some(TokenKind::EqEq) => BinOp::Eq,
                Some(TokenKind::BangEq) => BinOp::Ne,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_cmp();
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_cmp(&mut self) -> Expr {
        let mut lhs = self.parse_add();
        loop {
            let op = match self.peek() {
                Some(TokenKind::Lt) => BinOp::Lt,
                Some(TokenKind::Gt) => BinOp::Gt,
                Some(TokenKind::Le) => BinOp::Le,
                Some(TokenKind::Ge) => BinOp::Ge,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_add();
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_add(&mut self) -> Expr {
        let mut lhs = self.parse_mul();
        loop {
            let op = match self.peek() {
                Some(TokenKind::Plus) => BinOp::Add,
                Some(TokenKind::Minus) => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_mul();
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_mul(&mut self) -> Expr {
        let mut lhs = self.parse_unary();
        loop {
            let op = match self.peek() {
                Some(TokenKind::Star) => BinOp::Mul,
                Some(TokenKind::Slash) => BinOp::Div,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary();
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_unary(&mut self) -> Expr {
        if self.eat_if(TokenKind::Bang) {
            return Expr::UnaryNot(Box::new(self.parse_unary()));
        }
        if self.eat_if(TokenKind::Minus) {
            return match self.parse_unary() {
                Expr::Int(v) => Expr::Int(-v),
                Expr::Float(v) => Expr::Float(-v),
                Expr::Length(v, u) => Expr::Length(-v, u),
                other => Expr::UnaryNeg(Box::new(other)),
            };
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            if self.eat_if(TokenKind::Dot) {
                if self.peek() == Some(TokenKind::Ident) {
                    let field = self.bump().unwrap().text;
                    if self.eat_if(TokenKind::LParen) {
                        let args = self.parse_call_args();
                        expr = Expr::MethodCall(Box::new(expr), field, args);
                    } else {
                        expr = Expr::Member(Box::new(expr), field);
                    }
                }
            } else if self.eat_if(TokenKind::LBracket) {
                let idx_expr = self.parse_expr();
                self.expect(TokenKind::RBracket, "expected ']' closing index expression");
                expr = Expr::Index(Box::new(expr), Box::new(idx_expr));
            } else {
                break;
            }
        }
        expr
    }

    fn parse_call_args(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        while let Some(pk) = self.peek() {
            if pk == TokenKind::RParen {
                break;
            }
            args.push(self.parse_expr());
            if !self.eat_if(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "expected ')' after arguments");
        args
    }

    fn parse_primary(&mut self) -> Expr {
        match self.peek() {
            Some(TokenKind::KwTrue) => {
                self.bump();
                Expr::Bool(true)
            }
            Some(TokenKind::KwFalse) => {
                self.bump();
                Expr::Bool(false)
            }
            Some(TokenKind::ColorHex) => {
                let t = self.bump().unwrap();
                Expr::ColorHex(t.text)
            }
            Some(TokenKind::Dimension) => {
                let t = self.bump().unwrap();
                parse_dimension(&t.text)
            }
            Some(TokenKind::Duration) => {
                let t = self.bump().unwrap();
                parse_duration(&t.text)
            }
            Some(TokenKind::Float) => {
                let t = self.bump().unwrap();
                Expr::Float(t.text.parse().unwrap_or(0.0))
            }
            Some(TokenKind::Int) => {
                let t = self.bump().unwrap();
                Expr::Int(t.text.parse().unwrap_or(0))
            }
            Some(TokenKind::StringLit) => {
                let t = self.bump().unwrap();
                Expr::String(unquote(&t.text))
            }
            Some(TokenKind::Ident) => {
                let name = self.bump().unwrap().text;
                if self.eat_if(TokenKind::LParen) {
                    let args = self.parse_call_args();
                    Expr::Call(name, args)
                } else if self.peek() == Some(TokenKind::LBrace)
                    && (self.peek_nth(1) == Some(TokenKind::RBrace)
                        || (self.peek_nth(1) == Some(TokenKind::Ident)
                            && self.peek_nth(2) == Some(TokenKind::Colon)))
                {
                    self.bump(); // {
                    let mut fields = Vec::new();
                    while let Some(pk) = self.peek() {
                        if pk == TokenKind::RBrace {
                            break;
                        }
                        if pk == TokenKind::Ident {
                            let fname = self.bump().unwrap().text;
                            self.expect(TokenKind::Colon, "expected ':' in struct literal field");
                            let val = self.parse_expr();
                            fields.push((fname, val));
                            let _ = self.eat_if(TokenKind::Comma) || self.eat_if(TokenKind::Semi);
                        } else {
                            break;
                        }
                    }
                    self.expect(TokenKind::RBrace, "expected '}' closing struct literal");
                    Expr::StructLit { name, fields }
                } else {
                    Expr::Ident(name)
                }
            }
            Some(TokenKind::LBracket) => {
                self.bump();
                let mut items = Vec::new();
                while let Some(pk) = self.peek() {
                    if pk == TokenKind::RBracket {
                        break;
                    }
                    items.push(self.parse_expr());
                    if !self.eat_if(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RBracket, "expected ']' closing array");
                Expr::Array(items)
            }
            Some(TokenKind::LParen) => {
                self.bump();
                let e = self.parse_expr();
                self.expect(TokenKind::RParen, "expected ')'");
                e
            }
            Some(TokenKind::AtLinearGradient) => {
                self.bump();
                self.expect(TokenKind::LParen, "expected '(' after @linear-gradient");
                let args = self.parse_call_args();
                Expr::Call("@linear-gradient".to_string(), args)
            }
            Some(TokenKind::AtRadialGradient) => {
                self.bump();
                self.expect(TokenKind::LParen, "expected '(' after @radial-gradient");
                let args = self.parse_call_args();
                Expr::Call("@radial-gradient".to_string(), args)
            }
            Some(TokenKind::At) => {
                self.bump();
                if self.peek() == Some(TokenKind::Ident) {
                    let name = format!("@{}", self.bump().unwrap().text);
                    if self.eat_if(TokenKind::LParen) {
                        let args = self.parse_call_args();
                        Expr::Call(name, args)
                    } else {
                        Expr::Ident(name)
                    }
                } else {
                    let off = self.current_offset();
                    self.diagnostics.push(Diagnostic {
                        span: Span::new(off, off + 1),
                        severity: Severity::Error,
                        message: "expected macro or builtin name after '@'".into(),
                    });
                    Expr::Bool(false)
                }
            }
            _ => {
                let off = self.current_offset();
                let len = self.bump().map(|t| t.text.len()).unwrap_or(1).max(1);
                self.diagnostics.push(Diagnostic {
                    span: Span::new(off, off + len),
                    severity: Severity::Error,
                    message: "expected expression".into(),
                });
                Expr::Bool(false)
            }
        }
    }
}

fn unquote(s: &str) -> String {
    let inner = s
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(s);
    inner
        .replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\\", "\\")
}

fn parse_dimension(s: &str) -> Expr {
    if let Some(num) = s.strip_suffix("dp") {
        Expr::Length(num.parse().unwrap_or(0.0), Unit::Dp)
    } else if let Some(num) = s.strip_suffix("px") {
        Expr::Length(num.parse().unwrap_or(0.0), Unit::Px)
    } else if let Some(num) = s.strip_suffix("deg") {
        Expr::Length(num.parse().unwrap_or(0.0), Unit::Deg)
    } else if let Some(num) = s.strip_suffix("rad") {
        Expr::Length(num.parse().unwrap_or(0.0), Unit::Rad)
    } else if let Some(num) = s.strip_suffix('%') {
        Expr::Length(num.parse().unwrap_or(0.0), Unit::Pct)
    } else {
        Expr::Length(0.0, Unit::Dp)
    }
}

fn parse_duration(s: &str) -> Expr {
    if let Some(num) = s.strip_suffix("ms") {
        Expr::DurationSec(num.parse::<f32>().unwrap_or(0.0) / 1000.0)
    } else if let Some(num) = s.strip_suffix('s') {
        Expr::DurationSec(num.parse::<f32>().unwrap_or(0.0))
    } else {
        Expr::DurationSec(0.0)
    }
}
