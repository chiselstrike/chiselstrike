use crate::filtering::FilterProperties;
use crate::query::Operator as QOperator;
use crate::symbols::Symbols;
use crate::transforms::utils::{extract_filter, lookup_callee_entity_type};
use crate::utils::{is_call_to_entity_method, is_ident_member_prop};

use swc_ecmascript::ast::{CallExpr, Callee, Expr};

pub mod emit;
pub mod splitting;

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
    extract_filter(call_expr, entity_type, "__filter".to_string())
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
