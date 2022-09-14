// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::filtering::FilterProperties;
use crate::query::BinaryExpr as QBinaryExpr;
use crate::query::BinaryOp as QBinaryOp;
use crate::query::Expr as QExpr;
use crate::query::Filter as QFilter;
use crate::query::Operator as QOperator;
use crate::query::PropertyAccessExpr as QPropertyAccessExpr;
use crate::query::Scan as QScan;
use crate::query::Value as QValue;
use crate::transforms::filter::splitting::{rewrite_filter_arrow, split_and_convert_expr};
use crate::utils::pat_to_string;
use anyhow::Result;

use swc_ecmascript::ast::{
    BinExpr, BinaryOp, BlockStmtOrExpr, CallExpr, Callee, Expr, Ident, Lit, MemberExpr, MemberProp,
    Stmt,
};

pub fn lookup_callee_entity_type(callee: &Callee) -> Result<String> {
    match callee {
        Callee::Expr(expr) => lookup_entity_type(expr),
        _ => {
            todo!();
        }
    }
}

fn lookup_entity_type(expr: &Expr) -> Result<String> {
    match expr {
        Expr::Member(MemberExpr { obj, .. }) => lookup_entity_type(obj),
        Expr::Call(CallExpr { callee, .. }) => lookup_callee_entity_type(callee),
        Expr::Ident(Ident { sym, .. }) => Ok(sym.to_string()),
        _ => {
            anyhow::bail!("Failed to look up entity type from call chain")
        }
    }
}

pub fn extract_filter(
    call_expr: &CallExpr,
    entity_type: String,
    function: String,
) -> (Option<Box<QOperator>>, Option<FilterProperties>) {
    let args = &call_expr.args;
    if args.len() != 1 {
        return (None, None);
    }
    let arg = &args[0];
    let arrow = match &*arg.expr {
        Expr::Arrow(arrow_expr) => arrow_expr,
        Expr::Object(object_lit) => {
            /*
             * Filter by restriction object, nothing to transform, but let's
             * grab predicate indexes.
             */
            let props = FilterProperties::from_object_lit(entity_type, object_lit);
            return (None, props);
        }
        _ => {
            /* filter() call that has a parameter type we don't recognize, nothing to transform.  */
            return (None, None);
        }
    };
    let params = &arrow.params;
    if params.len() != 1 {
        return (None, None);
    }
    let param = &params[0];
    let param = pat_to_string(param).unwrap();
    let (query, post_expr) = match &arrow.body {
        BlockStmtOrExpr::BlockStmt(block_stmt) => {
            if block_stmt.stmts.len() != 1 {
                return (None, None);
            }
            let return_stmt = match &block_stmt.stmts[0] {
                Stmt::Return(return_stmt) => return_stmt,
                _ => {
                    return (None, None);
                }
            };
            match &return_stmt.arg {
                Some(expr) => split_and_convert_expr(expr),
                None => {
                    todo!();
                }
            }
        }
        BlockStmtOrExpr::Expr(expr) => split_and_convert_expr(expr),
    };
    let (query_expr, predicate) = match query {
        Some(query) => query,
        None => return (None, None),
    };
    let query_expr = Box::new(rewrite_filter_arrow(arrow, query_expr));
    let post_expr = post_expr.map(|post_expr| Box::new(rewrite_filter_arrow(arrow, post_expr)));
    let filter = QFilter {
        function,
        call_expr: call_expr.to_owned(),
        query_expr,
        post_expr,
        parameters: vec![param.clone()],
        input: Box::new(QOperator::Scan(QScan {
            entity_type: entity_type.clone(),
            alias: param,
        })),
        predicate,
    };
    let props = FilterProperties::from_filter(entity_type, &filter);
    (Some(Box::new(QOperator::Filter(filter))), props)
}

pub fn convert_filter_expr(expr: &Expr) -> Option<QExpr> {
    match expr {
        Expr::Bin(bin_expr) => convert_bin_expr(bin_expr),
        Expr::Lit(Lit::Bool(value)) => Some(QExpr::Value(QValue::Bool(value.value))),
        _ => None,
    }
}

pub fn convert_bin_expr(expr: &BinExpr) -> Option<QExpr> {
    let left = convert_expr(&expr.left);
    let op = convert_binary_op(&expr.op);
    let right = convert_expr(&expr.right);
    if let (Some(left), Some(op), Some(right)) = (left, op, right) {
        Some(QExpr::BinaryExpr(QBinaryExpr {
            left: Box::new(left),
            op,
            right: Box::new(right),
        }))
    } else {
        None
    }
}

fn convert_expr(expr: &Expr) -> Option<QExpr> {
    match expr {
        Expr::Bin(bin_expr) => convert_bin_expr(bin_expr),
        Expr::Paren(paren_expr) => convert_expr(&*paren_expr.expr),
        Expr::Lit(Lit::Num(number)) => Some(QExpr::Value(QValue::Num(number.value))),
        Expr::Lit(Lit::Str(s)) => Some(QExpr::Value(QValue::Str(format!("{}", s.value)))),
        Expr::Member(member_expr) => {
            let obj = convert_expr(&member_expr.obj)?;
            let prop = match &member_expr.prop {
                MemberProp::Ident(ident) => ident.sym.to_string(),
                _ => {
                    todo!();
                }
            };
            Some(QExpr::PropertyAccess(QPropertyAccessExpr {
                object: Box::new(obj),
                property: prop,
            }))
        }
        Expr::Ident(ident) => Some(QExpr::Identifier(ident.sym.to_string())),
        _ => None,
    }
}

pub fn convert_binary_op(op: &BinaryOp) -> Option<QBinaryOp> {
    match op {
        BinaryOp::EqEq => Some(QBinaryOp::Eq),
        BinaryOp::Gt => Some(QBinaryOp::Gt),
        BinaryOp::GtEq => Some(QBinaryOp::GtEq),
        BinaryOp::Lt => Some(QBinaryOp::Lt),
        BinaryOp::LtEq => Some(QBinaryOp::LtEq),
        BinaryOp::NotEq => Some(QBinaryOp::NotEq),
        BinaryOp::LogicalAnd => Some(QBinaryOp::And),
        BinaryOp::LogicalOr => Some(QBinaryOp::Or),
        _ => None,
    }
}
