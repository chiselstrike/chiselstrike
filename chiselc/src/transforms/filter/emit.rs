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
use crate::query::Operator;
use crate::query::PropertyAccessExpr;
use crate::query::Value as QValue;
use swc_atoms::JsWord;
use swc_common::Span;
use swc_ecmascript::ast::Number;
use swc_ecmascript::ast::{
    Bool, CallExpr, Callee, Expr, ExprOrSpread, Ident, KeyValueProp, Lit, MemberProp, ObjectLit,
    Prop, PropName, PropOrSpread, Str,
};

/// Emit AST expression from query expression `operator`.
pub fn to_ts_expr(filter: &Operator) -> CallExpr {
    match filter {
        Operator::Filter(filter) => {
            /*
             * A filter consists of a query expression and an optional post
             * expression (with possible side-effects). As we transform a
             * `filter()` method call, for example, to an internal
             * `__filter()` method call, we pass the query expression as the
             * `exprPredicate` parameter and the post expression as
             * `postPredicate`, which is guaranteed to be always evaluated at
             * runtime -- and therefore causing any potential side-effects.
             */
            let callee = rewrite_filter_callee(&filter.call_expr.callee, &filter.function);
            let expr = filter_to_ts(filter, filter.call_expr.span);
            let expr = ExprOrSpread {
                spread: None,
                expr: Box::new(expr),
            };
            let mut args = vec![
                ExprOrSpread {
                    spread: None,
                    expr: filter.query_expr.clone(),
                },
                expr,
            ];
            if let Some(post) = &filter.post_expr {
                args.push(ExprOrSpread {
                    spread: None,
                    expr: post.clone(),
                })
            }
            CallExpr {
                span: filter.call_expr.span,
                callee,
                args,
                type_args: None,
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
        QExpr::Value(val) => value_to_ts(val, span),
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
        props.push(make_expr_type("Value", span));
        let lit = PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
            key: PropName::Ident(Ident {
                span,
                sym: JsWord::from("value"),
                optional: false,
            }),
            value: Box::new(Expr::Ident(Ident {
                span,
                sym: JsWord::from(ident),
                optional: false,
            })),
        })));
        props.push(lit);
    }
    Expr::Object(ObjectLit { span, props })
}

fn value_to_ts(lit: &QValue, span: Span) -> Expr {
    let mut props = vec![make_expr_type("Value", span)];
    let lit = match lit {
        QValue::Bool(v) => make_bool_lit(*v, span),
        QValue::Str(s) => make_str_lit(s, span),
        QValue::Num(n) => make_num_lit(n, span),
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
