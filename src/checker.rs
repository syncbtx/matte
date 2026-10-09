use std::collections::HashMap;

use bumpalo::Bump;
use logos::Span;

use crate::ast::core::{BinaryOp, ExprKind, Expression, Item, Program};
use crate::diagnostics::ParserError;

// ---------- types ----------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type<'a> {
    Num,
    Bool,
    Unit,
    Fn(&'a Type<'a>, &'a Type<'a>),
    Var(usize),
}

impl<'a> Type<'a> {
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        let mut names = Vec::new();
        self.write(&mut out, &mut names, false);
        out
    }

    fn write(&self, out: &mut String, names: &mut Vec<usize>, parens: bool) {
        match self {
            Type::Num => out.push_str("Num"),
            Type::Bool => out.push_str("Bool"),
            Type::Unit => out.push_str("Unit"),
            Type::Var(v) => {
                let i = match names.iter().position(|n| n == v) {
                    Some(i) => i,
                    None => {
                        names.push(*v);
                        names.len() - 1
                    }
                };
                out.push_str(&var_name(i));
            }
            Type::Fn(arg, ret) => {
                if parens {
                    out.push('(');
                }
                arg.write(out, names, true);
                out.push_str(" -> ");
                ret.write(out, names, false);
                if parens {
                    out.push(')');
                }
            }
        }
    }
}

fn var_name(i: usize) -> String {
    let letter = (b'a' + (i % 26) as u8) as char;
    if i < 26 {
        letter.to_string()
    } else {
        format!("{letter}{}", i / 26)
    }
}

// ---------- result ----------

#[derive(Debug)]
pub struct Checked<'a> {
    pub bindings: Vec<(&'a str, Type<'a>)>,
    pub typeofs: Vec<(usize, String)>,
    pub node_types: HashMap<usize, Type<'a>>,
}

impl<'a> Checked<'a> {
    pub fn print(&self) {
        self.bindings
            .iter()
            .for_each(|(name, ty)| println!("{name} :: {}", ty.pretty()));
        self.typeofs
            .iter()
            .for_each(|(_, s)| println!("typeof: {s}"));
    }
}

enum UnifyError {
    Mismatch,
    Infinite,
}

struct Global<'a> {
    ty: Type<'a>,
    /// Set once the binding's value has been inferred. Until then only
    /// function bodies may refer to it.
    defined: bool,
}

pub struct TypeChecker<'a> {
    arena: &'a Bump,
    subst: Vec<Option<Type<'a>>>,
    globals: HashMap<&'a str, Global<'a>>,
    /// Function parameters in scope, innermost last.
    locals: Vec<(&'a str, Type<'a>)>,
    fn_depth: usize,
    /// `==` / `<>` operand types, checked once types are resolved.
    eq_checks: Vec<(Type<'a>, Span)>,
    /// `print e;` argument types, checked once types are resolved.
    print_checks: Vec<(Type<'a>, Span)>,
    typeof_types: Vec<(usize, Type<'a>)>,
    node_types: HashMap<usize, Type<'a>>,
    errors: Vec<ParserError<'a>>,
}

impl<'a> TypeChecker<'a> {
    pub fn new(arena: &'a Bump) -> Self {
        Self {
            arena,
            subst: Vec::new(),
            globals: HashMap::new(),
            locals: Vec::new(),
            fn_depth: 0,
            eq_checks: Vec::new(),
            print_checks: Vec::new(),
            typeof_types: Vec::new(),
            node_types: HashMap::new(),
            errors: Vec::new(),
        }
    }

    /// Expects a desugared program (no `Item::FnBinding`).
    pub fn check(
        mut self,
        program: &Program<'a>,
    ) -> std::result::Result<Checked<'a>, Vec<ParserError<'a>>> {
        // Pass 1: declare every top-level name with a fresh type variable, so
        // function bodies can refer to names defined later (recursion).
        let mut skip = vec![false; program.items.len()];
        for (idx, item) in program.items.iter().enumerate() {
            if let Item::Binding { name, .. } = item {
                if self.globals.contains_key(name.name) {
                    self.error(
                        &name.span,
                        format!("`{}` is already defined", name.name),
                        "redefinition",
                    );
                    skip[idx] = true;
                } else {
                    let ty = self.fresh();
                    self.globals
                        .insert(name.name, Global { ty, defined: false });
                }
            }
        }

        // Pass 2: infer items top to bottom.
        let mut bindings = Vec::new();
        for (idx, item) in program.items.iter().enumerate() {
            match item {
                Item::Binding { name, value } => {
                    if skip[idx] {
                        continue;
                    }
                    let ty = self.infer(*value);
                    let declared = self.globals[name.name].ty;
                    self.expect(&ty, &declared, &value.span);
                    self.globals.get_mut(name.name).unwrap().defined = true;
                    bindings.push((name.name, declared));
                }
                Item::Expr(expr) => {
                    self.infer(*expr);
                }
                Item::Print { type_of, expr } => {
                    let ty = self.infer(*expr);
                    if *type_of {
                        self.typeof_types.push((idx, ty));
                    } else {
                        self.print_checks.push((ty, expr.span.clone()));
                    }
                }
                Item::FnBinding { .. } => unreachable!("desugar removes FnBinding"),
            }
        }

        // Pass 3: checks that need fully resolved types.
        for (ty, span) in std::mem::take(&mut self.eq_checks) {
            match self.resolve(&ty) {
                Type::Fn(..) | Type::Unit => {
                    let shown = self.resolve(&ty).pretty();
                    self.error(
                        &span,
                        format!("`==` and `<>` only compare Num or Bool, found `{shown}`"),
                        "cannot compare this type",
                    );
                }
                _ => {}
            }
        }
        for (ty, span) in std::mem::take(&mut self.print_checks) {
            let resolved = self.resolve(&ty);
            if !matches!(resolved, Type::Num | Type::Bool) {
                let shown = resolved.pretty();
                self.error(
                    &span,
                    format!("cannot print a value of type `{shown}`"),
                    "only Num and Bool can be printed",
                );
            }
        }

        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        let typeofs = std::mem::take(&mut self.typeof_types)
            .into_iter()
            .map(|(idx, ty)| (idx, self.resolve(&ty).pretty()))
            .collect();
        let bindings = bindings
            .into_iter()
            .map(|(name, ty)| (name, self.resolve(&ty)))
            .collect();

        for s in self.subst.iter_mut() {
            if s.is_none() {
                *s = Some(Type::Num);
            }
        }
        let node_types = std::mem::take(&mut self.node_types)
            .into_iter()
            .map(|(k, t)| (k, self.resolve(&t)))
            .collect();

        Ok(Checked {
            bindings,
            typeofs,
            node_types,
        })
    }

    // ---------- inference ----------
    //

    fn infer(&mut self, e: &'a Expression<'a>) -> Type<'a> {
        let ty = self.infer_inner(e);
        self.node_types
            .insert(e as *const Expression<'a> as usize, ty);
        ty
    }

    fn infer_inner(&mut self, e: &'a Expression<'a>) -> Type<'a> {
        match &e.kind {
            ExprKind::Unit => Type::Unit,
            ExprKind::Num(_) => Type::Num,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Ident(name) => self.lookup(*name, &e.span),

            // Neg and Fact both map Num -> Num.
            ExprKind::Unary { expr, .. } => {
                let t = self.infer(*expr);
                self.expect(&t, &Type::Num, &expr.span);
                Type::Num
            }

            ExprKind::BinaryOp { left, op, right } => match op {
                BinaryOp::Add(_)
                | BinaryOp::Sub(_)
                | BinaryOp::Mul(_)
                | BinaryOp::Div(_)
                | BinaryOp::Mod(_)
                | BinaryOp::Pow(_) => {
                    self.expect_num(*left);
                    self.expect_num(*right);
                    Type::Num
                }
                BinaryOp::Lt(_) | BinaryOp::Gt(_) | BinaryOp::Le(_) | BinaryOp::Ge(_) => {
                    self.expect_num(*left);
                    self.expect_num(*right);
                    Type::Bool
                }
                BinaryOp::Eq(_) | BinaryOp::Ne(_) => {
                    let l = self.infer(*left);
                    let r = self.infer(*right);
                    self.expect(&r, &l, &right.span);
                    self.eq_checks.push((l, e.span.clone()));
                    Type::Bool
                }
            },

            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.infer(*cond);
                self.expect(&c, &Type::Bool, &cond.span);
                let t = self.infer(*then_branch);
                let f = self.infer(*else_branch);
                self.expect(&f, &t, &else_branch.span);
                t
            }

            ExprKind::Fn { arg, body } => {
                let param = self.fresh();
                self.locals.push((arg.name, param));
                self.fn_depth += 1;
                let ret = self.infer(*body);
                self.fn_depth -= 1;
                self.locals.pop();
                self.mk_fn(param, ret)
            }

            ExprKind::App { func, arg } => {
                let tf = self.infer(*func);
                let ta = self.infer(*arg);
                match self.shallow(&tf) {
                    Type::Fn(param, ret) => {
                        self.expect(&ta, param, &arg.span);
                        *ret
                    }
                    Type::Var(_) => {
                        let ret = self.fresh();
                        let want = self.mk_fn(ta, ret);
                        self.expect(&tf, &want, &func.span);
                        ret
                    }
                    other => {
                        let shown = self.resolve(&other).pretty();
                        self.error(
                            &func.span,
                            format!("cannot apply a value of type `{shown}`"),
                            "this is not a function",
                        );
                        self.fresh()
                    }
                }
            }
        }
    }

    fn expect_num(&mut self, e: &'a Expression<'a>) {
        let t = self.infer(e);
        self.expect(&t, &Type::Num, &e.span);
    }

    /// Locals first, then top-level names. A top-level name that is declared
    /// but not yet defined may only be used inside a function body.
    fn lookup(&mut self, name: &'a str, span: &Span) -> Type<'a> {
        if let Some((_, ty)) = self.locals.iter().rev().find(|(n, _)| *n == name) {
            return *ty;
        }
        let found = self.globals.get(name).map(|g| (g.ty, g.defined));
        match found {
            Some((ty, defined)) if defined || self.fn_depth > 0 => ty,
            Some(_) => {
                self.error(
                    span,
                    format!("`{name}` is used before its definition"),
                    "only function bodies may refer to later names",
                );
                self.fresh()
            }
            None => {
                self.error(span, format!("unknown name `{name}`"), "not defined");
                self.fresh()
            }
        }
    }

    // ---------- unification ----------

    fn fresh(&mut self) -> Type<'a> {
        self.subst.push(None);
        Type::Var(self.subst.len() - 1)
    }

    fn mk_fn(&self, arg: Type<'a>, ret: Type<'a>) -> Type<'a> {
        Type::Fn(self.arena.alloc(arg), self.arena.alloc(ret))
    }

    /// Follows variable bindings at the top of the type only.
    fn shallow(&self, t: &Type<'a>) -> Type<'a> {
        let mut cur = *t;
        while let Type::Var(v) = cur {
            match self.subst[v] {
                Some(next) => cur = next,
                None => break,
            }
        }
        cur
    }

    /// Applies the substitution all the way down.
    fn resolve(&self, t: &Type<'a>) -> Type<'a> {
        match self.shallow(t) {
            Type::Fn(a, b) => self.mk_fn(self.resolve(a), self.resolve(b)),
            other => other,
        }
    }

    fn occurs(&self, v: usize, t: &Type<'a>) -> bool {
        match self.shallow(t) {
            Type::Var(w) => v == w,
            Type::Fn(a, b) => self.occurs(v, a) || self.occurs(v, b),
            _ => false,
        }
    }

    fn unify(&mut self, a: &Type<'a>, b: &Type<'a>) -> std::result::Result<(), UnifyError> {
        let a = self.shallow(a);
        let b = self.shallow(b);
        match (a, b) {
            (Type::Num, Type::Num) | (Type::Bool, Type::Bool) | (Type::Unit, Type::Unit) => Ok(()),
            (Type::Var(x), Type::Var(y)) if x == y => Ok(()),
            (Type::Var(x), t) | (t, Type::Var(x)) => {
                if self.occurs(x, &t) {
                    Err(UnifyError::Infinite)
                } else {
                    self.subst[x] = Some(t);
                    Ok(())
                }
            }
            (Type::Fn(a1, b1), Type::Fn(a2, b2)) => {
                self.unify(a1, a2)?;
                self.unify(b1, b2)
            }
            _ => Err(UnifyError::Mismatch),
        }
    }

    /// Unifies `found` with `expected`, reporting at `span` on failure.
    fn expect(&mut self, found: &Type<'a>, expected: &Type<'a>, span: &Span) {
        match self.unify(found, expected) {
            Ok(()) => {}
            Err(UnifyError::Mismatch) => {
                let want = self.resolve(expected).pretty();
                let got = self.resolve(found).pretty();
                self.error(
                    span,
                    format!("type mismatch: expected `{want}`, found `{got}`"),
                    &format!("this has type `{got}`"),
                );
            }
            Err(UnifyError::Infinite) => self.error(
                span,
                "cannot construct an infinite type".to_string(),
                "this would make a type contain itself",
            ),
        }
    }

    fn error(&mut self, span: &Span, message: String, label: &str) {
        self.errors.push(ParserError(
            ariadne::Report::build(ariadne::ReportKind::Error, span.clone())
                .with_message(message)
                .with_label(ariadne::Label::new(span.clone()).with_message(label))
                .finish(),
        ));
    }
}
