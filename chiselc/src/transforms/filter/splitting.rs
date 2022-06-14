// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

//! Filter splitting optimization.
//!
//! The `predicate` in a `filter(precicate)` call can have side-effects, which prevent
//! the compiler from generating a query expression, because side-effects cannot be
//! transformed to SQL by the ChiselStrike runtime.
//!
//! This module implements a filter splitting optimization that allows the following
//! piece of code, for example:
//!
//! ```ignore
//! Person.cursor().filter(person => person.age > 40 && fetch("https://example.com"))
//! ```
//!
//! can be transformed into:
//!
//! ```ignore
//! Person.cursor()
//!       .filter(person => person.age > 40)
//!       .filter(person => fetch("https://example.com"))
//! ```
//!
//! which then allows the compiler to transform the `filter()` call with a pure
//! (no side-effects) predicate function into a query expression.

use swc_ecmascript::ast::{
    ArrowExpr, BinExpr, BinaryOp, BlockStmtOrExpr, Expr, MemberExpr, MemberProp, Stmt,
};

/// Splits an expression into pure and impure parts if possible.
pub fn split_expr(expr: &Expr) -> (Expr, Option<Expr>) {
    match expr {
        Expr::Bin(BinExpr {
            op: BinaryOp::LogicalAnd,
            left,
            right,
            span: _,
        }) => {
            if is_pure_expr(left) && !is_pure_expr(right) {
                (*left.to_owned(), Some(*right.to_owned()))
            } else {
                (expr.to_owned(), None)
            }
        }
        _ => (expr.to_owned(), None),
    }
}

/// Checks if the `expr` is pure (has no side-effects).
///
/// Note: we determine an expression to be pure conservatively. That is, we
/// assume a function call, for example, to always have side-effects.
fn is_pure_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Bin(bin_expr) => is_pure_expr(&bin_expr.left) && is_pure_expr(&bin_expr.right),
        Expr::Unary(unary_expr) => is_pure_expr(&unary_expr.arg),
        Expr::Member(member_expr) => is_pure_member_expr(member_expr),
        Expr::Lit(_) => true,
        _ => false,
    }
}

/// Checks if the `member_expr` is pure (has no side-effects).
fn is_pure_member_expr(member_expr: &MemberExpr) -> bool {
    matches!(&*member_expr.obj, Expr::Ident(_)) && matches!(&member_expr.prop, MemberProp::Ident(_))
}

/// Rewrite an filter arrow function to use `expr`.
///
/// We use this to retain the type signature of an arrow function passed to
/// `ChiselCursor.filter()` but replace the expression in case we have
/// performed filter splitting.
///
/// For example, consider the following:
///
/// ```ignore
/// Person.cursor().filter(person => person.age < 4 && fetch("www.example.com"))
/// ```
///
/// the _arrow_ part is:
///
/// ```
/// person => person.age < 4 && fetch("www.example.com")
/// ```
///
/// after filter splitting is performed we need two new arrows:
///
/// ```
/// person => person.age < 4
/// ```
///
/// and
///
/// ```
/// person => fetch("www.example.com")
/// ```
///
/// to do that, we basically substitute the original arrow with the
/// new split expressions. We know this is safe because the arrow
/// parameters remains the same, and we only split the expression (but
/// did not modify it) so the expressions will evaluate fine with the
/// original arrow parameters.
pub fn rewrite_filter_arrow(arrow: &ArrowExpr, expr: Expr) -> Expr {
    match &arrow.body {
        BlockStmtOrExpr::BlockStmt(block_stmt) => {
            assert_eq!(block_stmt.stmts.len(), 1);
            let return_stmt = match &block_stmt.stmts[0] {
                Stmt::Return(return_stmt) => return_stmt,
                _ => {
                    todo!();
                }
            };
            match &return_stmt.arg {
                Some(_expr) => {
                    let mut return_stmt = return_stmt.clone();
                    return_stmt.arg = Some(Box::new(expr));
                    let mut block_stmt = block_stmt.clone();
                    block_stmt.stmts[0] = Stmt::Return(return_stmt);
                    let mut arrow = arrow.clone();
                    arrow.body = BlockStmtOrExpr::BlockStmt(block_stmt);
                    Expr::Arrow(arrow)
                }
                None => {
                    todo!();
                }
            }
        }
        BlockStmtOrExpr::Expr(_) => {
            let mut arrow = arrow.clone();
            arrow.body = BlockStmtOrExpr::Expr(Box::new(expr));
            Expr::Arrow(arrow)
        }
    }
}
