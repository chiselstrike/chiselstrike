use crate::symbols::Symbols;
use swc_ecmascript::ast::{Expr, MemberProp, Pat};

pub fn is_ident_member_prop(member_prop: &MemberProp, value: &str) -> bool {
    match member_prop {
        MemberProp::Ident(ident) => ident.sym == *value,
        _ => false,
    }
}

pub fn ident_to_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(ident) => Some(ident.sym.to_string()),
        _ => None,
    }
}

pub fn is_call_to_entity_method(expr: &Expr, method: &str, symbols: &Symbols) -> bool {
    match expr {
        Expr::Member(member_expr) => {
            let expr = &member_expr.obj;
            if let Some(ty) = ident_to_string(expr) {
                if !symbols.is_entity(&ty) {
                    return false;
                }
            }
            is_ident_member_prop(&member_expr.prop, method)
        }
        _ => false,
    }
}

pub fn pat_to_string(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(ident) => Some(ident.id.sym.to_string()),
        _ => None,
    }
}
