use std::collections::HashMap;

use bumpalo::Bump;
use logos::Span;

use crate::ast::core::{BinaryOp, UnaryOp};
use crate::diagnostics::ParserError;
use crate::resolve::{
    ExprId, GlobalId, LocalId, ResolvedExprKind, ResolvedItem, ResolvedName, ResolvedProgram,
};

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

#[derive(Debug, Clone)]
pub struct CheckedBinding<'a> {
    pub id: GlobalId,
    pub name: &'a str,
    pub ty: Type<'a>,
}

#[derive(Debug)]
pub struct Checked<'a> {
    pub bindings: Vec<CheckedBinding<'a>>,
    /// `(source item index, rendered type)` for `print typeof e;`.
    pub typeofs: Vec<(usize, String)>,
    /// Types are keyed by stable expression IDs, not raw AST addresses.
    pub node_types: HashMap<ExprId, Type<'a>>,
}

impl<'a> Checked<'a> {
    pub fn print(&self) {
        for binding in &self.bindings {
            println!("{} :: {}", binding.name, binding.ty.pretty());
        }
        for (_, ty) in &self.typeofs {
            println!("typeof: {ty}");
        }
    }

    pub fn type_of(&self, expr: ExprId) -> Option<&Type<'a>> {
        self.node_types.get(&expr)
    }
}

#[derive(Debug)]
enum UnifyError {
    Mismatch,
    Infinite,
}

/// Infers monomorphic types for a name-resolved, desugared program.
///
/// This checker intentionally does not implement Hindley–Milner let-polymorphism.
/// An unresolved type variable is reported as ambiguous rather than silently
/// defaulted to `Num`.
pub struct TypeChecker<'a> {
    arena: &'a Bump,
    subst: Vec<Option<Type<'a>>>,
    var_spans: Vec<Span>,
    globals: Vec<Type<'a>>,
    locals: Vec<Option<Type<'a>>>,
    eq_checks: Vec<(Type<'a>, Span)>,
    print_checks: Vec<(Type<'a>, Span)>,
    typeof_types: Vec<(usize, Type<'a>)>,
    node_types: HashMap<ExprId, Type<'a>>,
    errors: Vec<ParserError<'a>>,
}

impl<'a> TypeChecker<'a> {
    pub fn new(arena: &'a Bump) -> Self {
        Self {
            arena,
            subst: Vec::new(),
            var_spans: Vec::new(),
            globals: Vec::new(),
            locals: Vec::new(),
            eq_checks: Vec::new(),
            print_checks: Vec::new(),
            typeof_types: Vec::new(),
            node_types: HashMap::new(),
            errors: Vec::new(),
        }
    }

    pub fn check(
        mut self,
        program: &ResolvedProgram<'a>,
    ) -> Result<Checked<'a>, Vec<ParserError<'a>>> {
        // Allocate a type variable for every declared global before inferring
        // any body. This supports recursion and references between functions.
        for global in program.globals {
            let ty = self.fresh(&global.name_span);
            self.globals.push(ty);
        }
        self.locals = vec![None; program.local_count];

        let mut bindings = Vec::new();

        // Infer in source order. Name resolution has already validated which
        // references are legal at each point in the program.
        for (item_index, item) in program.items.iter().enumerate() {
            match item {
                ResolvedItem::Binding {
                    id,
                    name,
                    name_span,
                    value,
                } => {
                    let inferred = self.infer(value);
                    let declared = self.global_type(*id, name_span);
                    self.expect(&inferred, &declared, &value.span);
                    bindings.push((*id, *name, name_span.clone(), declared));
                }
                ResolvedItem::Expr(expr) => {
                    self.infer(expr);
                }
                ResolvedItem::Print { type_of, expr } => {
                    let ty = self.infer(expr);
                    if *type_of {
                        self.typeof_types.push((item_index, ty));
                    } else {
                        self.print_checks.push((ty, expr.span.clone()));
                    }
                }
            }
        }

        // Constraints whose validity depends on the final substitutions.
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
                Type::Num | Type::Bool | Type::Var(_) => {}
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

        // This is a monomorphic checker: every type variable that remains
        // unconstrained is an ambiguity. Do not silently turn it into Num.
        if self.errors.is_empty() {
            let unresolved_spans: Vec<Span> = self
                .subst
                .iter()
                .enumerate()
                .filter_map(|(id, value)| value.is_none().then(|| self.var_spans[id].clone()))
                .collect();
            for span in unresolved_spans {
                self.error(
                    &span,
                    "cannot infer a concrete type for this expression".to_string(),
                    "type remains unconstrained",
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
            .map(|(id, name, _span, ty)| CheckedBinding {
                id,
                name,
                ty: self.resolve(&ty),
            })
            .collect();

        let raw_node_types = std::mem::take(&mut self.node_types);
        let node_types = raw_node_types
            .into_iter()
            .map(|(id, ty)| (id, self.resolve(&ty)))
            .collect();

        Ok(Checked {
            bindings,
            typeofs,
            node_types,
        })
    }

    // ---------- inference ----------

    fn infer(&mut self, expr: &crate::resolve::ResolvedExpression<'a>) -> Type<'a> {
        let ty = self.infer_inner(expr);
        self.node_types.insert(expr.id, ty);
        ty
    }

    fn infer_inner(&mut self, expr: &crate::resolve::ResolvedExpression<'a>) -> Type<'a> {
        match &expr.kind {
            ResolvedExprKind::Unit => Type::Unit,
            ResolvedExprKind::Num(_) => Type::Num,
            ResolvedExprKind::Bool(_) => Type::Bool,
            ResolvedExprKind::Ident(name) => match name {
                ResolvedName::Global(id) => self.global_type(*id, &expr.span),
                ResolvedName::Local(id) => self.local_type(*id, &expr.span),
                ResolvedName::Error => self.fresh(&expr.span),
            },
            ResolvedExprKind::Unary { op, expr: inner } => {
                // Both negation and factorial are Num -> Num.
                match op {
                    UnaryOp::Neg(_) | UnaryOp::Fact(_) => {}
                }
                let actual = self.infer(inner);
                self.expect(&actual, &Type::Num, &inner.span);
                Type::Num
            }
            ResolvedExprKind::Binary { left, op, right } => match op {
                BinaryOp::Add(_)
                | BinaryOp::Sub(_)
                | BinaryOp::Mul(_)
                | BinaryOp::Div(_)
                | BinaryOp::Mod(_)
                | BinaryOp::Pow(_) => {
                    self.expect_num(left);
                    self.expect_num(right);
                    Type::Num
                }
                BinaryOp::Lt(_) | BinaryOp::Gt(_) | BinaryOp::Le(_) | BinaryOp::Ge(_) => {
                    self.expect_num(left);
                    self.expect_num(right);
                    Type::Bool
                }
                BinaryOp::Eq(_) | BinaryOp::Ne(_) => {
                    let left_ty = self.infer(left);
                    let right_ty = self.infer(right);
                    self.expect(&right_ty, &left_ty, &right.span);
                    self.eq_checks.push((left_ty, expr.span.clone()));
                    Type::Bool
                }
            },
            ResolvedExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let condition_ty = self.infer(cond);
                self.expect(&condition_ty, &Type::Bool, &cond.span);
                let then_ty = self.infer(then_branch);
                let else_ty = self.infer(else_branch);
                self.expect(&else_ty, &then_ty, &else_branch.span);
                then_ty
            }
            ResolvedExprKind::Fn {
                arg,
                arg_span,
                body,
                ..
            } => {
                let param_ty = self.fresh(arg_span);
                if let Some(slot) = self.locals.get_mut(arg.0) {
                    *slot = Some(param_ty);
                }
                let return_ty = self.infer(body);
                self.mk_fn(param_ty, return_ty)
            }
            ResolvedExprKind::App { func, arg } => {
                let func_ty = self.infer(func);
                let arg_ty = self.infer(arg);
                match self.shallow(&func_ty) {
                    Type::Fn(param, ret) => {
                        self.expect(&arg_ty, param, &arg.span);
                        *ret
                    }
                    Type::Var(_) => {
                        let ret_ty = self.fresh(&expr.span);
                        let wanted = self.mk_fn(arg_ty, ret_ty);
                        self.expect(&func_ty, &wanted, &func.span);
                        ret_ty
                    }
                    other => {
                        let shown = self.resolve(&other).pretty();
                        self.error(
                            &func.span,
                            format!("cannot apply a value of type `{shown}`"),
                            "this is not a function",
                        );
                        self.fresh(&expr.span)
                    }
                }
            }
        }
    }

    fn expect_num(&mut self, expr: &crate::resolve::ResolvedExpression<'a>) {
        let ty = self.infer(expr);
        self.expect(&ty, &Type::Num, &expr.span);
    }

    fn global_type(&mut self, id: GlobalId, span: &Span) -> Type<'a> {
        match self.globals.get(id.0).copied() {
            Some(ty) => ty,
            None => self.fresh(span),
        }
    }

    fn local_type(&mut self, id: LocalId, span: &Span) -> Type<'a> {
        match self.locals.get(id.0).copied().flatten() {
            Some(ty) => ty,
            None => self.fresh(span),
        }
    }

    // ---------- unification ----------

    fn fresh(&mut self, span: &Span) -> Type<'a> {
        self.subst.push(None);
        self.var_spans.push(span.clone());
        Type::Var(self.subst.len() - 1)
    }

    fn mk_fn(&self, arg: Type<'a>, ret: Type<'a>) -> Type<'a> {
        Type::Fn(self.arena.alloc(arg), self.arena.alloc(ret))
    }

    /// Follows substitutions at the outermost type constructor only.
    fn shallow(&self, ty: &Type<'a>) -> Type<'a> {
        let mut current = *ty;
        while let Type::Var(id) = current {
            match self.subst[id] {
                Some(next) => current = next,
                None => break,
            }
        }
        current
    }

    /// Fully applies the current substitution.
    fn resolve(&self, ty: &Type<'a>) -> Type<'a> {
        match self.shallow(ty) {
            Type::Fn(arg, ret) => self.mk_fn(self.resolve(arg), self.resolve(ret)),
            other => other,
        }
    }

    fn occurs(&self, needle: usize, ty: &Type<'a>) -> bool {
        match self.shallow(ty) {
            Type::Var(id) => needle == id,
            Type::Fn(arg, ret) => self.occurs(needle, arg) || self.occurs(needle, ret),
            Type::Num | Type::Bool | Type::Unit => false,
        }
    }

    fn unify(&mut self, a: &Type<'a>, b: &Type<'a>) -> Result<(), UnifyError> {
        let a = self.shallow(a);
        let b = self.shallow(b);
        match (a, b) {
            (Type::Num, Type::Num) | (Type::Bool, Type::Bool) | (Type::Unit, Type::Unit) => Ok(()),
            (Type::Var(x), Type::Var(y)) if x == y => Ok(()),
            (Type::Var(x), ty) | (ty, Type::Var(x)) => {
                if self.occurs(x, &ty) {
                    Err(UnifyError::Infinite)
                } else {
                    self.subst[x] = Some(ty);
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

    /// Unifies `found` with `expected`, reporting errors at `span`.
    fn expect(&mut self, found: &Type<'a>, expected: &Type<'a>, span: &Span) {
        match self.unify(found, expected) {
            Ok(()) => {}
            Err(UnifyError::Mismatch) => {
                let wanted = self.resolve(expected).pretty();
                let got = self.resolve(found).pretty();
                self.error(
                    span,
                    format!("type mismatch: expected `{wanted}`, found `{got}`"),
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
