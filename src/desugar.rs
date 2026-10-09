use bumpalo::Bump;
use logos::Span;

use crate::ast::core::{ExprKind, Expression, Identifier, Item, Program};
use crate::diagnostics::ParserError;

/// Rewrites sugar nodes into core ones. Today that is one rewrite:
///
///   FnBinding { target: `max x y`, value: e }
///     =>  Binding { name: max, value: fn x -> fn y -> e }
///
/// Input and output are the same AST type. The difference is the invariant:
/// after `run`, no `Item::FnBinding` remains. Bad binding heads are collected,
/// so one run reports every one of them.
pub struct Desugarer<'a> {
    arena: &'a Bump,
    errors: Vec<ParserError<'a>>,
}

impl<'a> Desugarer<'a> {
    pub fn new(arena: &'a Bump) -> Self {
        Self {
            arena,
            errors: Vec::new(),
        }
    }

    pub fn run(
        mut self,
        program: Program<'a>,
    ) -> std::result::Result<Program<'a>, Vec<ParserError<'a>>> {
        let mut items = Vec::with_capacity(program.items.len());
        for item in program.items {
            if let Some(item) = self.item(item) {
                items.push(item);
            }
        }
        if self.errors.is_empty() {
            Ok(Program { items })
        } else {
            Err(self.errors)
        }
    }

    fn item(&mut self, item: Item<'a>) -> Option<Item<'a>> {
        match item {
            Item::FnBinding { target, value } => self.fn_binding(target, value),
            // Expr, Print and Binding already hold core expressions.
            other => Some(other),
        }
    }

    fn fn_binding(
        &mut self,
        target: &'a Expression<'a>,
        value: &'a Expression<'a>,
    ) -> Option<Item<'a>> {
        let (name, params) = self.split_head(target)?;

        // Wrap innermost-first, so `max x y :: e` becomes fn x -> fn y -> e.
        // The outermost Fn spans from the start of the whole head.
        let mut body = value;
        for (i, arg) in params.into_iter().enumerate().rev() {
            let start = if i == 0 {
                target.span.start
            } else {
                arg.span.start
            };
            let span = Span {
                start,
                end: body.span.end,
            };
            body = self.arena.alloc(Expression {
                kind: ExprKind::Fn { arg, body },
                span,
            });
        }

        Some(Item::Binding { name, value: body })
    }

    /// Splits `f a b` (= App(App(f, a), b)) into `f` and `[a, b]`, requiring
    /// every part to be a plain identifier. Keeps walking after a bad
    /// argument so all bad parts are reported.
    fn split_head(
        &mut self,
        target: &'a Expression<'a>,
    ) -> Option<(Identifier<'a>, Vec<Identifier<'a>>)> {
        let mut cur = target;
        let mut params = Vec::new();
        let mut ok = true;

        while let ExprKind::App { func, arg } = &cur.kind {
            match arg.kind {
                ExprKind::Ident(name) => params.push(Identifier {
                    name,
                    span: arg.span.clone(),
                }),
                _ => {
                    self.error(
                        arg.span.clone(),
                        "function parameter must be an identifier",
                        "not an identifier",
                    );
                    ok = false;
                }
            }
            cur = *func;
        }

        let head = match cur.kind {
            ExprKind::Ident(name) => Some(Identifier {
                name,
                span: cur.span.clone(),
            }),
            _ => {
                self.error(
                    cur.span.clone(),
                    "function name must be an identifier",
                    "not an identifier",
                );
                None
            }
        };

        // The spine is walked outside-in, so parameters were collected last-first.
        params.reverse();
        if ok { head.map(|h| (h, params)) } else { None }
    }

    fn error(&mut self, span: Span, message: &str, label: &str) {
        self.errors.push(ParserError(
            ariadne::Report::build(ariadne::ReportKind::Error, span.clone())
                .with_message(message)
                .with_label(ariadne::Label::new(span).with_message(label))
                .finish(),
        ));
    }
}
