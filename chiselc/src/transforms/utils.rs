use crate::query::BinaryExpr as QBinaryExpr;
use crate::query::BinaryOp as QBinaryOp;
use crate::query::Expr as QExpr;
use crate::query::Literal as QLiteral;
use crate::query::PropertyAccessExpr as QPropertyAccessExpr;
use anyhow::{anyhow, Result};

use swc_ecmascript::ast::{
    BinExpr, BinaryOp, CallExpr, Callee, Expr, Ident, Lit, MemberExpr, MemberProp,
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

pub fn convert_bin_expr(expr: &BinExpr) -> Result<QExpr> {
    let left = Box::new(convert_expr(&expr.left)?);
    let op = convert_binary_op(&expr.op)?;
    let right = Box::new(convert_expr(&expr.right)?);
    Ok(QExpr::BinaryExpr(QBinaryExpr { left, op, right }))
}

fn convert_expr(expr: &Expr) -> Result<QExpr> {
    match expr {
        Expr::Bin(bin_expr) => convert_bin_expr(bin_expr),
        Expr::Paren(paren_expr) => Ok(convert_expr(&*paren_expr.expr)?),
        Expr::Lit(Lit::Num(number)) => Ok(QExpr::Literal(QLiteral::Num(number.value))),
        Expr::Lit(Lit::Str(s)) => Ok(QExpr::Literal(QLiteral::Str(format!("{}", s.value)))),
        Expr::Member(member_expr) => {
            let obj = convert_expr(&member_expr.obj)?;
            let prop = match &member_expr.prop {
                MemberProp::Ident(ident) => ident.sym.to_string(),
                _ => {
                    todo!();
                }
            };
            Ok(QExpr::PropertyAccess(QPropertyAccessExpr {
                object: Box::new(obj),
                property: prop,
            }))
        }
        Expr::Ident(ident) => Ok(QExpr::Identifier(ident.sym.to_string())),
        _ => Err(anyhow!("Unsupported expression: {:#?}", expr)),
    }
}

pub fn convert_binary_op(op: &BinaryOp) -> Result<QBinaryOp> {
    Ok(match op {
        BinaryOp::EqEq => QBinaryOp::Eq,
        BinaryOp::Gt => QBinaryOp::Gt,
        BinaryOp::GtEq => QBinaryOp::GtEq,
        BinaryOp::Lt => QBinaryOp::Lt,
        BinaryOp::LtEq => QBinaryOp::LtEq,
        BinaryOp::NotEq => QBinaryOp::NotEq,
        BinaryOp::LogicalAnd => QBinaryOp::And,
        BinaryOp::LogicalOr => QBinaryOp::Or,
        _ => {
            anyhow::bail!("Cannot convert binary operator {}", op);
        }
    })
}
