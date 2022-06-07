use crate::filtering::FilterProperties;
use crate::query::Expr as QExpr;
use crate::query::Filter as QFilter;
use crate::query::Literal as QLiteral;
use crate::query::Operator as QOperator;
use crate::query::Scan as QScan;
use crate::symbols::Symbols;
use crate::transforms::utils::{convert_bin_expr, lookup_callee_entity_type};
use crate::utils::{is_call_to_entity_method, is_ident_member_prop, pat_to_string};

use swc_ecmascript::ast::{BlockStmtOrExpr, CallExpr, Callee, Expr, Lit, Stmt};

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
    extract_filter(call_expr, entity_type, "__filterWithExpression".to_string())
}

fn extract_filter(
    call_expr: &CallExpr,
    entity_type: String,
    function: String,
) -> (Option<Box<QOperator>>, Option<FilterProperties>) {
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
        function,
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
            Expr::Member(member_expr) if is_ident_member_prop(&member_expr.prop, "filter") => {
                match &*member_expr.obj {
                    Expr::Call(call_expr) => match &call_expr.callee {
                        Callee::Expr(expr) => is_call_to_entity_method(expr, "cursor", symbols),
                        _ => false,
                    },
                    _ => false,
                }
            }
            _ => false,
        },
        _ => false,
    }
}
