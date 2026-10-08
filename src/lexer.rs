use std::fmt::{self, Display};

use logos::{Logos, Span};

#[derive(Logos, Debug, Copy, Clone, PartialEq)]
#[logos(skip(r"[ \t\r\n\f]+"))]
#[logos(skip(r"--[^\n]*", allow_greedy = true))]
pub enum TokenKind<'a> {
    #[token("fn")]
    Fn,
    #[token("if")]
    If,
    #[token("then")]
    Then,
    #[token("else")]
    Else,
    #[token("print")]
    Print,
    #[token("typeof")]
    TypeOf,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("()")]
    Unit,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token(":")]
    Colon,
    #[token("::")]
    DoubleColon,
    #[token(";")]
    SemiColon,
    #[token("?")]
    Question,
    #[token("->")]
    Arrow,
    #[token("@@")]
    DoubleAt,

    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("^")]
    Caret,

    #[token("==")]
    Eq,
    #[token("<>")]
    Ne,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("!")]
    Bang,

    #[regex(r"[0-9]+(\.)?[0-9]*([eE][+-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok())]
    Num(f64),

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice())]
    Ident(&'a str),
}

impl Display for TokenKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Num(n) => write!(f, "Num:{n}"),
            TokenKind::Ident(s) => write!(f, "Ident:{s}"),
            other => write!(f, "{other:?}"),
        }
    }
}

#[derive(Debug)]
pub struct Token<'a> {
    pub kind: TokenKind<'a>,
    pub span: Span,
}

impl Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{:?} @ {}..{}",
            self.kind, self.span.start, self.span.end
        )
    }
}

#[derive(Debug)]
pub struct LexerErr<'a> {
    pub src: &'a str,
    pub span: Span,
}

pub fn tokenize(src: &'_ str) -> Result<Vec<Token<'_>>, LexerErr<'_>> {
    let lexer = TokenKind::lexer(src);

    let mut tokens = Vec::new();

    for (res, span) in lexer.spanned() {
        match res {
            Ok(kind) => tokens.push(Token { kind, span }),
            Err(()) => return Err(LexerErr { src, span }),
        }
    }

    Ok(tokens)
}
