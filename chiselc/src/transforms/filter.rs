use crate::filtering::FilterProperties;
use crate::query::BinaryExpr as QBinaryExpr;
use crate::query::BinaryOp as QBinaryOp;
use crate::query::Expr as QExpr;
use crate::query::Filter as QFilter;
use crate::query::Literal as QLiteral;
use crate::query::Operator as QOperator;
use crate::query::PropertyAccessExpr as QPropertyAccessExpr;
use crate::query::Scan as QScan;
use crate::symbols::Symbols;
use crate::utils::{is_call_to_entity_cursor, is_ident_member_prop, pat_to_string};
use anyhow::{anyhow, Result};

use swc_ecmascript::ast::{
    BinExpr, BinaryOp, BlockStmtOrExpr, CallExpr, Callee, Expr, Ident, Lit, MemberExpr, MemberProp,
    Stmt,
};

/// Infer filter operator from the lambda predicate of to filter()
pub fn infer_filter(
    call_expr: &CallExpr,
    symbols: &Symbols,
) -> (Option<Box<QOperator>>, Option<FilterProperties>) {
    if !is_rewritable_filter(&call_expr.callee, symbols) {
        return (None, None);
    }
    let entity_type = match lookup_callee_entity_type(&call_expr.callee) {
        Ok(entity_type) => entity_type,
        _ => return (None, None),
    };
    let args = &call_expr.args;
    assert_eq!(args.len(), 1);
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
    assert_eq!(params.len(), 1);
    let param = &params[0];
    let param = pat_to_string(param).unwrap();
    let expr = match &arrow.body {
        BlockStmtOrExpr::BlockStmt(block_stmt) => {
            assert_eq!(block_stmt.stmts.len(), 1);
            let return_stmt = match &block_stmt.stmts[0] {
                Stmt::Return(return_stmt) => return_stmt,
                _ => {
                    return (None, None);
                }
            };
            match &return_stmt.arg {
                Some(expr) => match &**expr {
                    Expr::Bin(bin_expr) => convert_bin_expr(bin_expr),
                    Expr::Lit(Lit::Bool(value)) => Ok(QExpr::Literal(QLiteral::Bool(value.value))),
                    _ => {
                        todo!();
                    }
                },
                None => {
                    todo!();
                }
            }
        }
        BlockStmtOrExpr::Expr(expr) => match &**expr {
            Expr::Bin(bin_expr) => convert_bin_expr(bin_expr),
            _ => todo!("Unsupported filter predicate expression: {:?}", expr),
        },
    };
    let expr = match expr {
        Ok(expr) => expr,
        Err(_) => return (None, None),
    };
    let filter = QFilter {
        parameters: vec![param.clone()],
        input: Box::new(QOperator::Scan(QScan {
            entity_type: entity_type.clone(),
            alias: param,
        })),
        predicate: expr,
    };
    let props = FilterProperties::from_filter(entity_type, &filter);
    (Some(Box::new(QOperator::Filter(filter))), props)
}

fn is_rewritable_filter(callee: &Callee, symbols: &Symbols) -> bool {
    match callee {
        Callee::Expr(expr) => match &**expr {
            Expr::Member(member_expr) => {
                is_ident_member_prop(&member_expr.prop, "filter")
                    && is_call_to_entity_cursor(&member_expr.obj, symbols)
            }
            _ => false,
        },
        _ => false,
    }
}

fn lookup_callee_entity_type(callee: &Callee) -> Result<String> {
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

fn convert_bin_expr(expr: &BinExpr) -> Result<QExpr> {
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

fn convert_binary_op(op: &BinaryOp) -> Result<QBinaryOp> {
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
