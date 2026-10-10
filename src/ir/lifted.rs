//! Lifted IR: no nested lambdas.
//!
//! Every lambda becomes a top-level `Func`. A lambda's captured variables are
//! its leading parameters, so "create the closure" is "partially apply the
//! lifted function to the captures". Calls are classified here:
//!
//! - `CallDirect`:  known function, exactly saturated.
//! - `MakeClosure`: known function, fewer args than its total arity
//!                  (also a bare function used as a value, with zero args).
//! - `Apply`:       unknown callee (a local, a closure stored in a global,
//!                  the result of a call); also the leftover args of an
//!                  over-saturated call.
//!
//! Nodes live in the bump arena, like the AST and types.
//!
//! Total arity of a `Func` is `captures.len() + params.len()`, and the args of
//! `CallDirect` / `MakeClosure` are given in that order (captures first).

use std::collections::HashMap;

use bumpalo::Bump;

use crate::checker::Type;
use crate::ir::core::{self as core, PrimOp};
use crate::resolve::{GlobalId, LocalId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub usize);

#[derive(Debug, Clone)]
pub struct Func<'a> {
    pub id: FuncId,
    pub name: &'a str,
    pub captures: &'a [(LocalId, Type<'a>)],
    pub params: &'a [(LocalId, Type<'a>)],
    pub ret: Type<'a>,
    pub body: Expr<'a>,
}

impl<'a> Func<'a> {
    pub fn arity(&self) -> usize {
        self.captures.len() + self.params.len()
    }
}

#[derive(Debug, Clone)]
pub struct Expr<'a> {
    pub kind: ExprKind<'a>,
    pub ty: Type<'a>,
}

#[derive(Debug, Clone)]
pub enum ExprKind<'a> {
    Unit,
    Num(f64),
    Bool(bool),
    Local(LocalId),
    /// Read of a non-function global's slot.
    Global(GlobalId),
    Prim {
        op: PrimOp,
        args: &'a [Expr<'a>],
    },
    If {
        cond: &'a Expr<'a>,
        then_branch: &'a Expr<'a>,
        else_branch: &'a Expr<'a>,
    },
    CallDirect {
        func: FuncId,
        args: &'a [Expr<'a>],
    },
    MakeClosure {
        func: FuncId,
        args: &'a [Expr<'a>],
    },
    Apply {
        callee: &'a Expr<'a>,
        args: &'a [Expr<'a>],
    },
}

#[derive(Debug, Clone)]
pub enum Stmt<'a> {
    SetGlobal(GlobalId, Expr<'a>),
    Eval(Expr<'a>),
    Print(Expr<'a>),
    PrintType(&'a str),
}

#[derive(Debug, Clone)]
pub struct Program<'a> {
    /// Indexed by `FuncId`.
    pub funcs: &'a [Func<'a>],
    /// Non-function globals that need a data slot, initialized by `main`.
    pub slots: &'a [(GlobalId, &'a str, Type<'a>)],
    /// Runs in order; this is the language's top-level initialization order.
    pub main: &'a [Stmt<'a>],
    /// Indexed by `GlobalId`.
    pub names: &'a [&'a str],
}

pub fn lift<'a>(arena: &'a Bump, program: &core::Program<'a>) -> Program<'a> {
    let mut lf = Lifter {
        arena,
        known: HashMap::new(),
        funcs: Vec::new(),
        next_func: 0,
    };

    // Pre-pass: every top-level lambda gets its FuncId and arity before any
    // body is lifted, so calls to later functions (and recursion) resolve.
    for item in program.items {
        if let core::Item::Global { id, value, .. } = item {
            if let core::ExprKind::Lam { params, .. } = &value.kind {
                lf.known.insert(*id, (FuncId(lf.next_func), params.len()));
                lf.next_func += 1;
            }
        }
    }

    let mut slots = Vec::new();
    let mut main = Vec::new();
    for item in program.items {
        match item {
            core::Item::Global { id, name, value } => match &value.kind {
                core::ExprKind::Lam { params, body } => {
                    let (fid, _) = lf.known[id];
                    let body = lf.expr(body);
                    lf.funcs.push(Func {
                        id: fid,
                        name: *name,
                        captures: &[],
                        params: *params,
                        ret: body.ty,
                        body,
                    });
                }
                _ => {
                    slots.push((*id, *name, value.ty));
                    let v = lf.expr(value);
                    main.push(Stmt::SetGlobal(*id, v));
                }
            },
            core::Item::Eval(e) => {
                let e = lf.expr(e);
                main.push(Stmt::Eval(e));
            }
            core::Item::Print(e) => {
                let e = lf.expr(e);
                main.push(Stmt::Print(e));
            }
            core::Item::PrintType(s) => main.push(Stmt::PrintType(*s)),
        }
    }

    lf.funcs.sort_by_key(|f| f.id.0);
    Program {
        funcs: arena.alloc_slice_fill_iter(lf.funcs),
        slots: arena.alloc_slice_fill_iter(slots),
        main: arena.alloc_slice_fill_iter(main),
        names: program.global_names,
    }
}

struct Lifter<'a> {
    arena: &'a Bump,
    /// Top-level function globals: (function, arity).
    known: HashMap<GlobalId, (FuncId, usize)>,
    funcs: Vec<Func<'a>>,
    next_func: usize,
}

impl<'a> Lifter<'a> {
    fn node(&self, e: Expr<'a>) -> &'a Expr<'a> {
        self.arena.alloc(e)
    }

    fn slice<T>(&self, items: Vec<T>) -> &'a [T] {
        self.arena.alloc_slice_fill_iter(items)
    }

    fn exprs_vec(&mut self, es: &[core::Expr<'a>]) -> Vec<Expr<'a>> {
        es.iter().map(|e| self.expr(e)).collect()
    }

    fn expr(&mut self, e: &core::Expr<'a>) -> Expr<'a> {
        let ty = e.ty;
        let kind = match &e.kind {
            core::ExprKind::Unit => ExprKind::Unit,
            core::ExprKind::Num(n) => ExprKind::Num(*n),
            core::ExprKind::Bool(b) => ExprKind::Bool(*b),
            core::ExprKind::Local(l) => ExprKind::Local(*l),
            core::ExprKind::Global(g) => match self.known.get(g) {
                // A top-level function used as a value: a closure with no args.
                Some(&(func, _)) => ExprKind::MakeClosure { func, args: &[] },
                None => ExprKind::Global(*g),
            },
            core::ExprKind::Prim { op, args } => {
                let args = self.exprs_vec(args);
                ExprKind::Prim {
                    op: *op,
                    args: self.slice(args),
                }
            }
            core::ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond = self.expr(cond);
                let then_branch = self.expr(then_branch);
                let else_branch = self.expr(else_branch);
                ExprKind::If {
                    cond: self.node(cond),
                    then_branch: self.node(then_branch),
                    else_branch: self.node(else_branch),
                }
            }
            core::ExprKind::Lam { .. } => {
                let (func, captured, _) = self.lift_lam(e);
                ExprKind::MakeClosure {
                    func,
                    args: self.slice(captured),
                }
            }
            core::ExprKind::App { func, args } => return self.app(func, args, ty),
        };
        Expr { kind, ty }
    }

    /// Lifts a (nested) lambda to a top-level function. Returns its id, the
    /// expressions for its captured variables (evaluated at the closure
    /// creation site), and its total arity.
    fn lift_lam(&mut self, lam: &core::Expr<'a>) -> (FuncId, Vec<Expr<'a>>, usize) {
        let core::ExprKind::Lam { params, body } = &lam.kind else {
            unreachable!("lift_lam is only called on lambdas");
        };

        // Free variables of the whole lambda, including those needed only by
        // lambdas nested inside it (so a middle lambda re-captures them).
        let mut free = Vec::new();
        free_vars(lam, &mut Vec::new(), &mut free);

        let lifted_body = self.expr(body);
        let id = FuncId(self.next_func);
        self.next_func += 1;

        let total = free.len() + params.len();
        let captured = free
            .iter()
            .map(|(l, t)| Expr {
                kind: ExprKind::Local(*l),
                ty: *t,
            })
            .collect();
        self.funcs.push(Func {
            id,
            name: self.arena.alloc_str(&format!("lam_{}", id.0)),
            captures: self.slice(free),
            params: *params,
            ret: lifted_body.ty,
            body: lifted_body,
        });
        (id, captured, total)
    }

    fn app(&mut self, func: &core::Expr<'a>, args: &[core::Expr<'a>], ty: Type<'a>) -> Expr<'a> {
        let args = self.exprs_vec(args);

        // A callee is "known" if we can name the function being called:
        // a top-level function, or a lambda applied on the spot.
        let known = match &func.kind {
            core::ExprKind::Global(g) if self.known.contains_key(g) => {
                let (f, arity) = self.known[g];
                Some((f, Vec::new(), arity))
            }
            core::ExprKind::Lam { .. } => Some(self.lift_lam(func)),
            _ => None,
        };

        let Some((f, prefix, total)) = known else {
            let callee = self.expr(func);
            return Expr {
                kind: ExprKind::Apply {
                    callee: self.node(callee),
                    args: self.slice(args),
                },
                ty,
            };
        };

        let prefix_len = prefix.len();
        let mut full = prefix;
        full.extend(args);

        if full.len() == total {
            Expr {
                kind: ExprKind::CallDirect {
                    func: f,
                    args: self.slice(full),
                },
                ty,
            }
        } else if full.len() < total {
            Expr {
                kind: ExprKind::MakeClosure {
                    func: f,
                    args: self.slice(full),
                },
                ty,
            }
        } else {
            // Over-saturated: call with exactly `total` args, apply the rest
            // to the returned closure.
            let rest = full.split_off(total);
            let inner_ty = peel(func.ty, total - prefix_len);
            let call = Expr {
                kind: ExprKind::CallDirect {
                    func: f,
                    args: self.slice(full),
                },
                ty: inner_ty,
            };
            Expr {
                kind: ExprKind::Apply {
                    callee: self.node(call),
                    args: self.slice(rest),
                },
                ty,
            }
        }
    }
}

/// The type left after applying `n` arguments to a value of type `ty`.
fn peel<'a>(mut ty: Type<'a>, n: usize) -> Type<'a> {
    for _ in 0..n {
        match ty {
            Type::Fn(_, ret) => ty = *ret,
            other => unreachable!("applied too many args to {other:?}"),
        }
    }
    ty
}

/// Free locals of `e`, in first-occurrence order, with their types. `LocalId`s
/// are unique per declaration, so no shadowing logic is needed.
fn free_vars<'a>(e: &core::Expr<'a>, bound: &mut Vec<LocalId>, out: &mut Vec<(LocalId, Type<'a>)>) {
    match &e.kind {
        core::ExprKind::Local(l) => {
            if !bound.contains(l) && !out.iter().any(|(x, _)| x == l) {
                out.push((*l, e.ty));
            }
        }
        core::ExprKind::Unit
        | core::ExprKind::Num(_)
        | core::ExprKind::Bool(_)
        | core::ExprKind::Global(_) => {}
        core::ExprKind::Prim { args, .. } => {
            args.iter().for_each(|a| free_vars(a, bound, out));
        }
        core::ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            free_vars(cond, bound, out);
            free_vars(then_branch, bound, out);
            free_vars(else_branch, bound, out);
        }
        core::ExprKind::Lam { params, body } => {
            let mark = bound.len();
            bound.extend(params.iter().map(|(l, _)| *l));
            free_vars(body, bound, out);
            bound.truncate(mark);
        }
        core::ExprKind::App { func, args } => {
            free_vars(func, bound, out);
            args.iter().for_each(|a| free_vars(a, bound, out));
        }
    }
}

// ---------- debug dump ----------

impl<'a> Program<'a> {
    pub fn dump(&self) -> String {
        let mut out = String::new();
        for f in self.funcs {
            let list = |xs: &[(LocalId, Type<'a>)]| {
                xs.iter()
                    .map(|(l, t)| format!("l{}: {}", l.0, t.pretty()))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push_str(&format!(
                "fn {}[{}]({}) -> {} =\n  {}\n",
                f.name,
                list(f.captures),
                list(f.params),
                f.ret.pretty(),
                self.show(&f.body)
            ));
        }
        for s in self.main {
            match s {
                Stmt::SetGlobal(g, e) => {
                    out.push_str(&format!("@{} := {}\n", self.names[g.0], self.show(e)))
                }
                Stmt::Eval(e) => out.push_str(&format!("{}\n", self.show(e))),
                Stmt::Print(e) => out.push_str(&format!("print {}\n", self.show(e))),
                Stmt::PrintType(t) => out.push_str(&format!("typeof: {t}\n")),
            }
        }
        out
    }

    fn show(&self, e: &Expr<'a>) -> String {
        let list = |xs: &[Expr<'a>]| {
            xs.iter()
                .map(|x| self.show(x))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match &e.kind {
            ExprKind::Unit => "()".to_string(),
            ExprKind::Num(n) => format!("{n}"),
            ExprKind::Bool(b) => format!("{b}"),
            ExprKind::Local(l) => format!("l{}", l.0),
            ExprKind::Global(g) => format!("@{}", self.names[g.0]),
            ExprKind::Prim { op, args } => match *args {
                [a] => format!("({op:?} {})", self.show(a)),
                [a, b] => format!("({} {op:?} {})", self.show(a), self.show(b)),
                _ => format!("{op:?}({})", list(args)),
            },
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => format!(
                "if {} then {} else {}",
                self.show(cond),
                self.show(then_branch),
                self.show(else_branch)
            ),
            ExprKind::CallDirect { func, args } => {
                format!("call {}({})", self.funcs[func.0].name, list(args))
            }
            ExprKind::MakeClosure { func, args } => {
                format!("closure {}[{}]", self.funcs[func.0].name, list(args))
            }
            ExprKind::Apply { callee, args } => {
                format!("apply {}({})", self.show(callee), list(args))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::core::BinaryOp;
    use crate::checker::Checked;
    use crate::ir::core::lower;
    use crate::resolve::{
        ExprId, GlobalSymbol, ResolvedExprKind as K, ResolvedExpression as RE, ResolvedItem,
        ResolvedName, ResolvedProgram,
    };
    use bumpalo::Bump;

    type T = Type<'static>;

    fn leak<X>(x: X) -> &'static X {
        Box::leak(Box::new(x))
    }

    fn fnty(a: T, b: T) -> T {
        Type::Fn(leak(a), leak(b))
    }

    /// Builds resolved expressions and records the type the checker would.
    struct B {
        next: usize,
        types: HashMap<ExprId, T>,
    }

    impl B {
        fn mk(&mut self, kind: K<'static>, ty: T) -> RE<'static> {
            let id = ExprId(self.next);
            self.next += 1;
            self.types.insert(id, ty);
            RE {
                id,
                kind,
                span: 0..0,
            }
        }
        fn num(&mut self, n: f64) -> RE<'static> {
            self.mk(K::Num(n), Type::Num)
        }
        fn local(&mut self, l: usize, ty: T) -> RE<'static> {
            self.mk(K::Ident(ResolvedName::Local(LocalId(l))), ty)
        }
        fn global(&mut self, g: usize, ty: T) -> RE<'static> {
            self.mk(K::Ident(ResolvedName::Global(GlobalId(g))), ty)
        }
        fn bin(&mut self, l: RE<'static>, op: BinaryOp, r: RE<'static>) -> RE<'static> {
            self.mk(
                K::Binary {
                    left: leak(l),
                    op,
                    right: leak(r),
                },
                Type::Num,
            )
        }
        fn lam(&mut self, arg: usize, param: T, body: RE<'static>) -> RE<'static> {
            let ty = fnty(param, self.types[&body.id]);
            self.mk(
                K::Fn {
                    arg: LocalId(arg),
                    arg_name: "_",
                    arg_span: 0..0,
                    body: leak(body),
                },
                ty,
            )
        }
        fn app(&mut self, f: RE<'static>, a: RE<'static>, ret: T) -> RE<'static> {
            self.mk(
                K::App {
                    func: leak(f),
                    arg: leak(a),
                },
                ret,
            )
        }
    }

    #[test]
    fn lowers_and_lifts() {
        let n = Type::Num;
        let nn = fnty(n, n);
        let nnn = fnty(n, nn);
        let twice_ty = fnty(nn, nn);
        let k_ty = fnty(n, nn);

        let mut b = B {
            next: 0,
            types: HashMap::new(),
        };

        // add :: fn x -> fn y -> x + y;               (g0, l0 l1)
        let x = b.local(0, n);
        let y = b.local(1, n);
        let sum = b.bin(x, BinaryOp::Add(0..0), y);
        let inner = b.lam(1, n, sum);
        let add = b.lam(0, n, inner);
        // inc :: add 1;                               (g1)
        let g_add = b.global(0, nnn);
        let one = b.num(1.0);
        let inc = b.app(g_add, one, nn);
        // twice :: fn f -> fn x -> f (f x);           (g2, l2 l3)
        let f1 = b.local(2, nn);
        let f2 = b.local(2, nn);
        let x3 = b.local(3, n);
        let fx = b.app(f2, x3, n);
        let ffx = b.app(f1, fx, n);
        let inner = b.lam(3, n, ffx);
        let twice = b.lam(2, nn, inner);
        // make :: fn x -> twice (fn y -> x * y);      (g3, l4 l5)
        let x4 = b.local(4, n);
        let y5 = b.local(5, n);
        let prod = b.bin(x4, BinaryOp::Mul(0..0), y5);
        let ylam = b.lam(5, n, prod);
        let g_twice = b.global(2, twice_ty);
        let tw = b.app(g_twice, ylam, nn);
        let make = b.lam(4, n, tw);
        // k :: fn x -> add x;                         (g4, l6)
        let g_add2 = b.global(0, nnn);
        let x6 = b.local(6, n);
        let addx = b.app(g_add2, x6, nn);
        let k = b.lam(6, n, addx);
        // twice inc 0;
        let g_twice2 = b.global(2, twice_ty);
        let g_inc = b.global(1, nn);
        let t1 = b.app(g_twice2, g_inc, nn);
        let zero = b.num(0.0);
        let t2 = b.app(t1, zero, n);
        // k 1 2;   (over-saturated)
        let g_k = b.global(4, k_ty);
        let one_b = b.num(1.0);
        let k1 = b.app(g_k, one_b, nn);
        let two = b.num(2.0);
        let k12 = b.app(k1, two, n);
        // print typeof add;
        let g_add3 = b.global(0, nnn);

        let bind = |id, name, value| ResolvedItem::Binding {
            id: GlobalId(id),
            name,
            name_span: 0..0,
            value,
        };
        let items = vec![
            bind(0, "add", add),
            bind(1, "inc", inc),
            bind(2, "twice", twice),
            bind(3, "make", make),
            bind(4, "k", k),
            ResolvedItem::Expr(t2),
            ResolvedItem::Expr(k12),
            ResolvedItem::Print {
                type_of: true,
                expr: g_add3,
            },
        ];
        let globals: Vec<GlobalSymbol<'static>> = ["add", "inc", "twice", "make", "k"]
            .iter()
            .enumerate()
            .map(|(i, name)| GlobalSymbol {
                id: GlobalId(i),
                name,
                name_span: 0..0,
                declaration_item: i,
            })
            .collect();
        let arena: &'static Bump = Box::leak(Box::new(Bump::new()));
        let resolved = ResolvedProgram {
            items: arena.alloc_slice_fill_iter(items),
            globals: arena.alloc_slice_fill_iter(globals),
            local_count: 7,
        };
        let checked = Checked {
            bindings: vec![],
            typeofs: vec![],
            node_types: b.types,
        };

        let core = lower(arena, &resolved, &checked);
        let lifted = lift(arena, &core);
        let dump = lifted.dump();
        println!("{dump}");

        // add, twice, make, k are top-level; make's inner lambda is lam_4.
        assert_eq!(lifted.funcs.len(), 5);
        assert_eq!(lifted.funcs[0].name, "add");
        assert_eq!(lifted.funcs[0].arity(), 2); // collapsed
        assert_eq!(lifted.funcs[1].arity(), 2);
        let lam = &lifted.funcs[4];
        assert_eq!(lam.captures.len(), 1); // captures x (l4)
        assert_eq!(lam.captures[0].0, LocalId(4));
        assert_eq!(lam.params.len(), 1);

        // inc is a data slot holding a closure; add is not.
        assert_eq!(lifted.slots.len(), 1);
        assert_eq!(lifted.slots[0].1, "inc");
        assert!(dump.contains("@inc := closure add[1]"));

        // twice inc 0  => saturated direct call, inc read from its slot.
        assert!(dump.contains("call twice(@inc, 0)"));
        // k 1 2  => direct call to k, then apply the rest.
        assert!(dump.contains("apply call k(1)(2)"));
        // make's body re-wraps the capture: twice [closure lam_4[x]]
        assert!(dump.contains("closure twice[closure lam_4[l4]]"));
        assert!(dump.contains("typeof: Num -> Num -> Num"));
    }
}
