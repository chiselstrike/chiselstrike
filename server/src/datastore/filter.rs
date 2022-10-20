use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, PropertyAccess, Value as ExprValue};
use anyhow::{Context, Result};

pub fn to_expr(e: &serde_json::Value) -> Result<Expr> {
    to_expr_rec(0, e)
}

fn to_expr_rec(depth: u32, e: &serde_json::Value) -> Result<Expr> {
    anyhow::ensure!(
        depth <= 100,
        "reached maximum expression filter recursion depth of 100"
    );

    let filter_obj = e.as_object().context("filter value is not an object")?;
    let mut expr: Expr = ExprValue::Bool(true).into();
    for (key, value) in filter_obj {
        let e = match key.as_str() {
            "$and" => {
                let mut expr: Expr = ExprValue::Bool(true).into();
                let operands = value
                    .as_array()
                    .context("operator $and must be used with an array, but got different type")?;
                for operand in operands {
                    expr = BinaryExpr::and(expr, to_expr_rec(depth + 1, operand)?)
                }
                expr
            }
            "$or" => {
                let mut expr = ExprValue::Bool(false).into();
                let operands = value
                    .as_array()
                    .context("operator $or must be used with an array, but got different type")?;
                for operand in operands {
                    expr = BinaryExpr::or(expr, to_expr_rec(depth + 1, operand)?)
                }
                expr
            }
            "$not" => Expr::Not(to_expr_rec(depth + 1, value)?.into()),
            field_name => {
                anyhow::ensure!(
                    !field_name.starts_with('$'),
                    "found an unknown filter operator '{field_name:?}'"
                );
                filed_filter_to_expr(field_name, value)?
            }
        };
        expr = BinaryExpr::and(expr, e);
    }
    Ok(expr)
}

fn filed_filter_to_expr(field_name: &str, value: &serde_json::Value) -> Result<Expr> {
    let property = PropertyAccess {
        property: field_name.to_owned(),
        object: Expr::Parameter { position: 0 }.into(),
    };
    field_filter_to_expr_rec(0, property, value)
}

fn field_filter_to_expr_rec(
    depth: u32,
    property: PropertyAccess,
    value: &serde_json::Value,
) -> Result<Expr> {
    anyhow::ensure!(
        depth <= 100,
        "reached maximum field filter recursion depth of 100"
    );
    if let Some(obj) = value.as_object() {
        let mut expr: Expr = ExprValue::Bool(true).into();
        for (key, value) in obj {
            let property_expr: Expr = property.clone().into();
            let field_filter = if key.starts_with('$') {
                let op = match key.as_str() {
                    "$eq" => BinaryOp::Eq,
                    "$ne" => BinaryOp::NotEq,
                    "$gt" => BinaryOp::Gt,
                    "$gte" => BinaryOp::GtEq,
                    "$lt" => BinaryOp::Lt,
                    "$lte" => BinaryOp::LtEq,
                    op_name => {
                        anyhow::bail!("encountered unknown comparison operator '{op_name:?}'")
                    }
                };
                let expr_value = ExprValue::try_from(value)?;
                BinaryExpr::new(op, property_expr, expr_value.into()).into()
            } else {
                let nested_field = PropertyAccess {
                    property: key.to_owned(),
                    object: property_expr.into(),
                };
                field_filter_to_expr_rec(depth + 1, nested_field, value)?
            };
            expr = BinaryExpr::and(expr, field_filter);
        }
        Ok(expr)
    } else {
        let expr_value = ExprValue::try_from(value)?;
        Ok(BinaryExpr::eq(property.into(), expr_value.into()))
    }
}
