// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

//! Filter emitting.
//!
//! The first compiler transforms AST to a query expression (the `Operator`
//! type), but we then need to transform that back to AST. This filter
//! emitting module does that.
//!
//! Please note that any time `ChiselEntity.find{Many,One}` or
//! `ChiselCursor.filter()` is changed, make sure to check if that has some
//! impact on this module or the `__find{Many.One}` and `__filter` internal
//! functions we emit calls to.

use crate::query::BinaryExpr as QBinaryExpr;
use crate::query::BinaryOp as QBinaryOp;
use crate::query::Expr as QExpr;
use crate::query::Filter;
use crate::query::Literal as QLiteral;
use crate::query::Operator;
use crate::query::PropertyAccessExpr;
use swc_atoms::JsWord;
use swc_common::Span;
use swc_ecmascript::ast::Number;
use swc_ecmascript::ast::{
    Bool, CallExpr, Callee, Expr, ExprOrSpread, Ident, KeyValueProp, Lit, MemberExpr, MemberProp,
    ObjectLit, Prop, PropName, PropOrSpread, Str,
};

/// Emit AST expression from query expression `operator`.
pub fn to_ts_expr(filter: &Operator) -> CallExpr {
    match filter {
        Operator::Filter(filter) => {
            /*
             * A filter consists of a pure expression (no side-effects) and an
             * optional impure expression (with possible side-effects). Each
             * part is transformed into a method call. The pure expression is
             * transformed into a `__filter()` call (that the runtime
             * optimizes) and the impure part is transformed into a normal
             * `filter()` call that is evaluated at runtime.
             */
            let pure_callee = rewrite_filter_callee(&filter.call_expr.callee, &filter.function);
            let expr = filter_to_ts(filter, filter.call_expr.span);
            let expr = ExprOrSpread {
                spread: None,
                expr: Box::new(expr),
            };
            let pure_args = vec![
                ExprOrSpread {
                    spread: None,
                    expr: filter.pure.clone(),
                },
                expr,
            ];
            let pure_call = CallExpr {
                span: filter.call_expr.span,
                callee: pure_callee,
                args: pure_args,
                type_args: None,
            };
            if let Some(impure) = &filter.impure {
                let impure_prop = MemberProp::Ident(Ident {
                    span: filter.call_expr.span,
                    sym: JsWord::from("filter"),
                    optional: false,
                });
                let impure_callee = Callee::Expr(Box::new(Expr::Member(MemberExpr {
                    span: filter.call_expr.span,
                    obj: Box::new(Expr::Call(pure_call)),
                    prop: impure_prop,
                })));
                let impure = ExprOrSpread {
                    spread: None,
                    expr: impure.clone(),
                };
                CallExpr {
                    span: filter.call_expr.span,
                    callee: impure_callee,
                    args: vec![impure],
                    type_args: None,
                }
            } else {
                pure_call
            }
        }
        _ => {
            todo!("TypeScript target only supports filtering.");
        }
    }
}

/// Rewrites the filter() call with __filter().
fn rewrite_filter_callee(callee: &Callee, function: &str) -> Callee {
    match callee {
        Callee::Expr(expr) => match &**expr {
            Expr::Member(member_expr) => {
                let mut member_expr = member_expr.clone();
                let prop = MemberProp::Ident(Ident {
                    span: member_expr.span,
                    sym: JsWord::from(function),
                    optional: false,
                });
                member_expr.prop = prop;
                Callee::Expr(Box::new(Expr::Member(member_expr)))
            }
            _ => {
                todo!();
            }
        },
        _ => {
            todo!();
        }
    }
}

fn filter_to_ts(filter: &Filter, span: Span) -> Expr {
    expr_to_ts(&filter.predicate, &filter.parameters, span)
}

fn expr_to_ts(expr: &QExpr, params: &[String], span: Span) -> Expr {
    match expr {
        QExpr::BinaryExpr(binary_expr) => binary_expr_to_ts(binary_expr, params, span),
        QExpr::PropertyAccess(property_access_expr) => {
            property_access_to_ts(property_access_expr, params, span)
        }
        QExpr::Identifier(ident) => identifier_to_ts(ident, params, span),
        QExpr::Literal(lit) => literal_to_ts(lit, span),
    }
}

fn binary_expr_to_ts(binary_expr: &QBinaryExpr, params: &[String], span: Span) -> Expr {
    let mut props = vec![make_expr_type("Binary", span)];
    let left = expr_to_ts(&binary_expr.left, params, span);
    let left = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
        key: PropName::Ident(Ident {
            span,
            sym: JsWord::from("left"),
            optional: false,
        }),
        value: Box::new(left),
    })));
    props.push(left);
    let op = binary_op_to_ts(&binary_expr.op, span);
    let op = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
        key: PropName::Ident(Ident {
            span,
            sym: JsWord::from("op"),
            optional: false,
        }),
        value: Box::new(op),
    })));
    props.push(op);
    let right = expr_to_ts(&binary_expr.right, params, span);
    let right = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
        key: PropName::Ident(Ident {
            span,
            sym: JsWord::from("right"),
            optional: false,
        }),
        value: Box::new(right),
    })));
    props.push(right);
    Expr::Object(ObjectLit { span, props })
}

fn binary_op_to_ts(binary_op: &QBinaryOp, span: Span) -> Expr {
    let raw_op = match binary_op {
        QBinaryOp::And => "And",
        QBinaryOp::Eq => "Eq",
        QBinaryOp::Gt => "Gt",
        QBinaryOp::GtEq => "GtEq",
        QBinaryOp::Lt => "Lt",
        QBinaryOp::LtEq => "LtEq",
        QBinaryOp::NotEq => "NotEq",
        QBinaryOp::Or => "Or",
    };
    make_str_lit(raw_op, span)
}

fn property_access_to_ts(
    property_access_expr: &PropertyAccessExpr,
    params: &[String],
    span: Span,
) -> Expr {
    let mut props = vec![make_expr_type("Property", span)];
    let obj = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
        key: PropName::Ident(Ident {
            span,
            sym: JsWord::from("object"),
            optional: false,
        }),
        value: Box::new(expr_to_ts(&property_access_expr.object, params, span)),
    })));
    props.push(obj);
    let prop = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
        key: PropName::Ident(Ident {
            span,
            sym: JsWord::from("property"),
            optional: false,
        }),
        value: Box::new(make_str_lit(&property_access_expr.property, span)),
    })));
    props.push(prop);
    Expr::Object(ObjectLit { span, props })
}

fn identifier_to_ts(ident: &str, params: &[String], span: Span) -> Expr {
    let mut props = vec![];
    if let Some(pos) = params.iter().position(|param| param == ident) {
        props.push(make_expr_type("Parameter", span));
        let lit = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
            key: PropName::Ident(Ident {
                span,
                sym: JsWord::from("position"),
                optional: false,
            }),
            value: Box::new(make_num_lit(&(pos as f64), span)),
        })));
        props.push(lit);
    } else {
        props.push(make_expr_type("Identifier", span));
        let lit = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
            key: PropName::Ident(Ident {
                span,
                sym: JsWord::from("ident"),
                optional: false,
            }),
            value: Box::new(make_str_lit(ident, span)),
        })));
        props.push(lit);
    }
    Expr::Object(ObjectLit { span, props })
}

fn literal_to_ts(lit: &QLiteral, span: Span) -> Expr {
    let mut props = vec![make_expr_type("Literal", span)];
    let lit = match lit {
        QLiteral::Bool(v) => make_bool_lit(*v, span),
        QLiteral::Str(s) => make_str_lit(s, span),
        QLiteral::Num(n) => make_num_lit(n, span),
    };
    let lit = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
        key: PropName::Ident(Ident {
            span,
            sym: JsWord::from("value"),
            optional: false,
        }),
        value: Box::new(lit),
    })));
    props.push(lit);
    Expr::Object(ObjectLit { span, props })
}

fn make_expr_type(expr_type: &str, span: Span) -> PropOrSpread {
    PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
        key: PropName::Ident(Ident {
            span,
            sym: JsWord::from("exprType"),
            optional: false,
        }),
        value: Box::new(make_str_lit(expr_type, span)),
    })))
}

fn make_bool_lit(value: bool, span: Span) -> Expr {
    Expr::Lit(Lit::Bool(Bool { span, value }))
}

fn make_str_lit(raw_str: &str, span: Span) -> Expr {
    Expr::Lit(Lit::Str(Str {
        span,
        value: JsWord::from(raw_str),
        raw: None,
    }))
}

fn make_num_lit(num: &f64, span: Span) -> Expr {
    Expr::Lit(Lit::Num(Number {
        span,
        value: *num,
        raw: None,
    }))
}
