use logos::Logos;

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum TokenKind {
    // Trivia (preserved in lossless rowan CST)
    #[regex(r"[ \t\r\n]+")]
    Whitespace = 0,

    #[regex(r"//[^\n]*", allow_greedy = true)]
    LineComment,

    #[regex(r"/\*([^*]|\*[^/])*\*/")]
    BlockComment,

    // Keywords
    #[token("import")]
    KwImport,
    #[token("from")]
    KwFrom,
    #[token("export")]
    KwExport,
    #[token("struct")]
    KwStruct,
    #[token("enum")]
    KwEnum,
    #[token("component")]
    KwComponent,
    #[token("in-out")]
    KwInOut,
    #[token("in")]
    KwIn,
    #[token("out")]
    KwOut,
    #[token("property")]
    KwProperty,
    #[token("callback")]
    KwCallback,
    #[token("for")]
    KwFor,
    #[token("if")]
    KwIf,
    #[token("else")]
    KwElse,
    #[token("true")]
    KwTrue,
    #[token("false")]
    KwFalse,

    // Multi-char operators
    #[token("<=>")]
    TwoWayBind,
    #[token("=>")]
    FatArrow,
    #[token("..=")]
    DotDotEq,
    #[token("..")]
    DotDot,
    #[token("==")]
    EqEq,
    #[token("!=")]
    BangEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,

    // Single-char punctuation & operators
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token(":")]
    Colon,
    #[token(";")]
    Semi,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("=")]
    Eq,
    #[token("!")]
    Bang,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("?")]
    Question,

    // Literals
    #[regex(r"#[0-9a-fA-F]{3,8}")]
    ColorHex,

    #[regex(r"[0-9]+(\.[0-9]+)?(dp|px|%|deg|rad)")]
    Dimension,

    #[regex(r"[0-9]+(\.[0-9]+)?(ms|s)")]
    Duration,

    #[regex(r"[0-9]+\.[0-9]+")]
    Float,

    #[regex(r"[0-9]+")]
    Int,

    #[regex(r#""([^"\\]|\\.)*""#)]
    StringLit,

    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Ident,

    #[token("@linear-gradient")]
    AtLinearGradient,

    #[token("@radial-gradient")]
    AtRadialGradient,

    #[token("@children")]
    AtChildren,

    #[token("@")]
    At,

    Error,
}

impl TokenKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub start: usize,
    pub end: usize,
}

pub fn tokenize(source: &str) -> Vec<Token> {
    let mut lexer = TokenKind::lexer(source);
    let mut tokens = Vec::new();
    while let Some(res) = lexer.next() {
        let span = lexer.span();
        let kind = res.unwrap_or(TokenKind::Error);
        tokens.push(Token {
            kind,
            text: source[span.clone()].to_string(),
            start: span.start,
            end: span.end,
        });
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_ranges_subtraction_and_negative_literals_cleanly() {
        let toks: Vec<(TokenKind, &str)> = tokenize("0..10 0..=n count-1 -16dp in-out")
            .into_iter()
            .filter(|t| !t.kind.is_trivia())
            .map(|t| (t.kind, Box::leak(t.text.into_boxed_str()) as &str))
            .collect();
        assert_eq!(
            toks,
            vec![
                (TokenKind::Int, "0"),
                (TokenKind::DotDot, ".."),
                (TokenKind::Int, "10"),
                (TokenKind::Int, "0"),
                (TokenKind::DotDotEq, "..="),
                (TokenKind::Ident, "n"),
                (TokenKind::Ident, "count"),
                (TokenKind::Minus, "-"),
                (TokenKind::Int, "1"),
                (TokenKind::Minus, "-"),
                (TokenKind::Dimension, "16dp"),
                (TokenKind::KwInOut, "in-out"),
            ]
        );
    }
}
