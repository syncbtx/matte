use std::iter::Peekable;

use bumpalo::Bump;
use logos::Span;
use vpratt::*;

use crate::ast::core::{BinaryOp, ExprKind, Expression, Identifier, Item, Program, UnaryOp};
use crate::diagnostics::ParserError;
use crate::lexer::{Token, TokenKind, TokenKind::*};

pub struct MatteParser<'a, I: Iterator<Item = Token<'a>>> {
    pub stream: Peekable<I>,
    pub arena: &'a Bump,
}

impl<'a, I: Iterator<Item = Token<'a>>> MatteParser<'a, I> {
    pub fn new(iter: I, arena: &'a Bump) -> Self {
        Self {
            stream: iter.peekable(),
            arena,
        }
    }

    pub fn parse(&mut self) -> std::result::Result<Program<'a>, ParserError<'a>> {
        let mut items = Vec::new();
        while self.stream.peek().is_some() {
            items.push(self.parse_item()?);
        }
        Ok(Program { items })
    }

    /// An item is one of:
    ///   print [typeof] expr;
    ///   expr;                       -> Item::Expr
    ///   name :: expr;               -> Item::Binding      (pure)
    ///   <application> :: expr;      -> Item::FnBinding    (sugar, validated by desugar)
    ///
    /// Everything that is not `print` starts as an ordinary expression. Only
    /// after seeing `::` do we know it was a binding head. A bare identifier
    /// head is a plain Binding; any other head (e.g. `max x y`) is kept as an
    /// unvalidated application target for the desugar pass.
    pub fn parse_item(&mut self) -> std::result::Result<Item<'a>, ParserError<'a>> {
        if matches!(self.stream.peek().map(|t| t.kind), Some(TokenKind::Print)) {
            self.stream.next();
            let type_of = matches!(self.stream.peek().map(|t| t.kind), Some(TokenKind::TypeOf));
            if type_of {
                self.stream.next();
            }
            let expr = self.arena.alloc(self.pratt_parse()?);
            self.expect_semicolon()?;
            return Ok(Item::Print { type_of, expr });
        }

        let lhs = self.arena.alloc(self.pratt_parse()?);

        if matches!(
            self.stream.peek().map(|t| t.kind),
            Some(TokenKind::DoubleColon)
        ) {
            self.stream.next();
            let value = self.arena.alloc(self.pratt_parse()?);
            self.expect_semicolon()?;
            return Ok(match lhs.kind {
                ExprKind::Ident(name) => Item::Binding {
                    name: Identifier {
                        name,
                        span: lhs.span.clone(),
                    },
                    value,
                },
                _ => Item::FnBinding { target: lhs, value },
            });
        }

        self.expect_semicolon()?;
        Ok(Item::Expr(lhs))
    }

    fn expect_semicolon(&mut self) -> std::result::Result<(), ParserError<'a>> {
        match self.stream.next() {
            Some(tok) if matches!(tok.kind, TokenKind::SemiColon) => Ok(()),
            Some(tok) => {
                let err = vpratt::VprattError::ExpectedTokenMismatch(TokenKind::SemiColon, tok);
                Err(err.into())
            }
            None => Err(vpratt::VprattError::UnexpectedEOF.into()),
        }
    }
}

#[vpratt::parser(
    stream = self.stream,
    item = Token<'a>,
    token = TokenKind<'a>,
    output = Expression<'a>,
    error = ParserError<'a>,
    extract = |token: &Token<'a>| token.kind.clone()
)]
#[allow(unused)]
impl<'a, I: Iterator<Item = Token<'a>>> MatteParser<'a, I> {
    // Binding power, low to high:
    //   @@ < ?: < comparison < + - < * / % < prefix - < ^ < application < postfix !
    const TABLE: vpratt::Table<Self> = vpratt::Table::new()
        .terminal(Ident(""), Self::terminals)
        .terminal(Num(0.0), Self::terminals)
        .terminal(True, Self::terminals)
        .terminal(False, Self::terminals)
        .terminal(Unit, Self::terminals)
        .group(LParen, RParen, Self::grouped)
        .structural(If, Self::parse_if)
        .structural(Fn, Self::parse_fn)
        .prefix(55, Minus, Self::neg)
        .postfix(90, Bang, Self::fact)
        .implied(70, vpratt::Associativity::Left, Ident(""), Self::app)
        .implied(70, vpratt::Associativity::Left, Num(0.0), Self::app)
        .implied(70, vpratt::Associativity::Left, True, Self::app)
        .implied(70, vpratt::Associativity::Left, False, Self::app)
        .implied(70, vpratt::Associativity::Left, Unit, Self::app)
        .implied(70, vpratt::Associativity::Left, LParen, Self::app)
        .implied(70, vpratt::Associativity::Left, If, Self::app)
        .implied(70, vpratt::Associativity::Left, Fn, Self::app)
        .infix(60, vpratt::Associativity::Right, Caret, Self::binary)
        .infix(50, vpratt::Associativity::Left, Star, Self::binary)
        .infix(50, vpratt::Associativity::Left, Slash, Self::binary)
        .infix(50, vpratt::Associativity::Left, Percent, Self::binary)
        .infix(40, vpratt::Associativity::Left, Plus, Self::binary)
        .infix(40, vpratt::Associativity::Left, Minus, Self::binary)
        .infix(30, vpratt::Associativity::Left, Eq, Self::binary)
        .infix(30, vpratt::Associativity::Left, Ne, Self::binary)
        .infix(30, vpratt::Associativity::Left, Lt, Self::binary)
        .infix(30, vpratt::Associativity::Left, Gt, Self::binary)
        .infix(30, vpratt::Associativity::Left, Le, Self::binary)
        .infix(30, vpratt::Associativity::Left, Ge, Self::binary)
        .infix(10, vpratt::Associativity::Right, Question, Self::ternary)
        .infix(5, vpratt::Associativity::Right, DoubleAt, Self::app_op);

    #[vpratt::handler]
    fn terminals(&mut self, ctx: TerminalCtx<Self>) -> vpratt::Result<Self> {
        let span = ctx.consumed.token.span;
        match ctx.consumed.token.kind {
            TokenKind::Ident(val) => Ok(Expression {
                kind: ExprKind::Ident(val),
                span,
            }),
            TokenKind::Num(val) => Ok(Expression {
                kind: ExprKind::Num(val),
                span,
            }),
            TokenKind::True => Ok(Expression {
                kind: ExprKind::Bool(true),
                span,
            }),
            TokenKind::False => Ok(Expression {
                kind: ExprKind::Bool(false),
                span,
            }),
            TokenKind::Unit => Ok(Expression {
                kind: ExprKind::Unit,
                span,
            }),
            _ => unreachable!(),
        }
    }

    #[vpratt::handler]
    fn grouped(&mut self, ctx: GroupCtx<Self>) -> vpratt::Result<Self> {
        Ok(ctx.enclosed.parse(self)?.0)
    }

    #[vpratt::handler]
    fn parse_if(&mut self, ctx: StructuralCtx<Self>) -> vpratt::Result<Self> {
        let start = ctx.consumed.token.span.start;
        let cond = self.arena.alloc(ctx.sub.parse(self)?);
        ctx.expect(self, Then)?;
        let then_branch = self.arena.alloc(ctx.sub.parse(self)?);
        ctx.expect(self, Else)?;
        let else_branch = self.arena.alloc(ctx.sub.parse(self)?);
        let end = else_branch.span.end;
        let kind = ExprKind::If {
            cond,
            then_branch,
            else_branch,
        };
        Ok(Expression {
            kind,
            span: Span { start, end },
        })
    }

    /// `fn x y z -> body` is nested right here into
    /// `Fn(x, Fn(y, Fn(z, body)))`, so the AST only ever holds
    /// single-parameter functions and no sugar node is needed.
    #[vpratt::handler]
    fn parse_fn(&mut self, ctx: StructuralCtx<Self>) -> vpratt::Result<Self> {
        let start = ctx.consumed.token.span.start;

        let first = ctx.expect(self, Ident(""))?;
        let Ident(name) = first.token.kind else {
            unreachable!()
        };
        let mut params = vec![Identifier {
            name,
            span: first.token.span,
        }];
        while let Some(next) = ctx.accept(self, Ident(""))? {
            let Ident(name) = next.token.kind else {
                unreachable!()
            };
            params.push(Identifier {
                name,
                span: next.token.span,
            });
        }

        ctx.expect(self, Arrow)?;
        let mut acc = ctx.sub.parse(self)?;

        // Wrap innermost-first. The outermost Fn spans from the `fn` keyword,
        // inner ones from their own parameter.
        for (i, arg) in params.into_iter().enumerate().rev() {
            let span_start = if i == 0 { start } else { arg.span.start };
            let end = acc.span.end;
            let body = self.arena.alloc(acc);
            acc = Expression {
                kind: ExprKind::Fn { arg, body },
                span: Span {
                    start: span_start,
                    end,
                },
            };
        }
        Ok(acc)
    }

    #[vpratt::handler]
    fn neg(&mut self, ctx: PrefixCtx<Self>) -> vpratt::Result<Self> {
        let start = ctx.consumed.token.span.start;
        let op = UnaryOp::Neg(ctx.consumed.token.span);
        let expr = self.arena.alloc(ctx.rhs.parse(self)?);
        Ok(Expression {
            kind: ExprKind::Unary { op, expr },
            span: Span {
                start,
                end: expr.span.end,
            },
        })
    }

    #[vpratt::handler]
    fn fact(&mut self, ctx: PostfixCtx<Self>) -> vpratt::Result<Self> {
        let end = ctx.consumed.token.span.end;
        let op = UnaryOp::Fact(ctx.consumed.token.span);
        let expr = self.arena.alloc(ctx.lhs);
        Ok(Expression {
            kind: ExprKind::Unary { op, expr },
            span: Span {
                start: expr.span.start,
                end,
            },
        })
    }

    #[vpratt::handler]
    fn binary(&mut self, ctx: InfixCtx<Self>) -> vpratt::Result<Self> {
        let left = self.arena.alloc(ctx.lhs);
        let op_span = ctx.consumed.token.span.clone();
        let is_cmp = matches!(ctx.consumed.token.kind, Eq | Ne | Lt | Gt | Le | Ge);
        let op = match ctx.consumed.token.kind {
            Plus => BinaryOp::Add(op_span),
            Minus => BinaryOp::Sub(op_span),
            Star => BinaryOp::Mul(op_span),
            Slash => BinaryOp::Div(op_span),
            Percent => BinaryOp::Mod(op_span),
            Caret => BinaryOp::Pow(op_span),
            Eq => BinaryOp::Eq(op_span),
            Ne => BinaryOp::Ne(op_span),
            Lt => BinaryOp::Lt(op_span),
            Gt => BinaryOp::Gt(op_span),
            Le => BinaryOp::Le(op_span),
            Ge => BinaryOp::Ge(op_span),
            _ => unreachable!(),
        };
        let right = self.arena.alloc(ctx.rhs.parse(self)?);

        // Comparisons are non-associative: `a < b < c` is a syntax error.
        if is_cmp {
            if let Some(next) = self.stream.peek() {
                if matches!(next.kind, Eq | Ne | Lt | Gt | Le | Ge) {
                    return Err(err(
                        next.span.clone(),
                        "comparison operators cannot be chained",
                        "parenthesise one of the comparisons",
                    ));
                }
            }
        }

        Ok(Expression {
            kind: ExprKind::Binary { left, op, right },
            span: Span {
                start: left.span.start,
                end: right.span.end,
            },
        })
    }

    #[vpratt::handler]
    fn ternary(&mut self, ctx: InfixCtx<Self>) -> vpratt::Result<Self> {
        let start = ctx.lhs.span.start;
        let cond = self.arena.alloc(ctx.lhs.clone());

        let mut branches = ctx.series_sub(self, Colon)?;
        let then_branch = self
            .arena
            .alloc(branches.pop().expect("expected then branch before colon"));

        ctx.expect(self, Colon)?;

        let else_branch = self.arena.alloc(ctx.rhs.parse(self)?);
        Ok(Expression {
            kind: ExprKind::If {
                cond,
                then_branch,
                else_branch,
            },
            span: Span {
                start,
                end: else_branch.span.end,
            },
        })
    }

    #[vpratt::handler]
    fn app(&mut self, ctx: ImpliedCtx<Self>) -> vpratt::Result<Self> {
        let start = ctx.lhs.span.start;
        let func = self.arena.alloc(ctx.lhs);

        let arg_initial = ctx.seed.parse(self, ctx.consumed.token)?;
        let arg_final = ctx.resume.parse(self, arg_initial)?;
        let arg = self.arena.alloc(arg_final);

        Ok(Expression {
            kind: ExprKind::App { func, arg },
            span: Span {
                start,
                end: arg.span.end,
            },
        })
    }

    #[vpratt::handler]
    fn app_op(&mut self, ctx: InfixCtx<Self>) -> vpratt::Result<Self> {
        let start = ctx.lhs.span.start;
        let func = self.arena.alloc(ctx.lhs);
        let arg = self.arena.alloc(ctx.rhs.parse(self)?);
        Ok(Expression {
            kind: ExprKind::App { func, arg },
            span: Span {
                start,
                end: arg.span.end,
            },
        })
    }
}

fn err<'a>(span: Span, message: &str, label: &str) -> ParserError<'a> {
    ParserError(
        ariadne::Report::build(ariadne::ReportKind::Error, span.clone())
            .with_message(message)
            .with_label(ariadne::Label::new(span).with_message(label))
            .finish(),
    )
}
