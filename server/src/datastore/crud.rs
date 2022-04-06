use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, Literal, PropertyAccess};
use crate::datastore::query::{QueryOp, SortBy};
use crate::types::{ObjectType, Type};
use crate::JsonObject;

use anyhow::{Context, Result};
use url::Url;

use std::sync::Arc;

pub(crate) fn query_str_to_ops(base_type: &Arc<ObjectType>, url: &str) -> Result<Vec<QueryOp>> {
    let q = Url::parse(url).with_context(|| format!("failed to parse query string '{}'", url))?;

    let mut limit: Option<QueryOp> = None;
    let mut offset: Option<QueryOp> = None;
    let mut ops = vec![];
    for (param_key, value) in q.query_pairs().into_owned() {
        let param_key = param_key.to_string();
        let op = parse_query_parameter(base_type, &param_key, &value).with_context(|| {
            format!(
                "failed to parse query param '{}' with value '{}'",
                param_key, value
            )
        })?;

        if let Some(QueryOp::Skip { .. }) = op {
            offset = op;
        } else if let Some(QueryOp::Take { .. }) = op {
            limit = op;
        } else if let Some(op) = op {
            ops.push(op);
        }
    }
    if let Some(offset) = offset {
        ops.push(offset);
    }
    if let Some(limit) = limit {
        ops.push(limit);
    }
    Ok(ops)
}

fn parse_query_parameter(
    base_type: &Arc<ObjectType>,
    param_key: &str,
    value: &str,
) -> Result<Option<QueryOp>> {
    let op = match param_key {
        "sort" => {
            let mut ascending = true;
            let field_name = if let Some(suffix) = value.strip_prefix(&['-', '+']) {
                if value.starts_with('-') {
                    ascending = false;
                }
                suffix
            } else {
                value
            };
            anyhow::ensure!(
                base_type.has_field(field_name),
                "trying to sort by non-existent field '{}' on entity {}",
                field_name,
                base_type.name(),
            );
            QueryOp::SortBy(SortBy {
                field_name: field_name.to_owned(),
                ascending,
            })
        }
        "limit" => {
            let count = value
                .parse()
                .with_context(|| format!("failed to parse limit. Expected u64, got '{}'", value))?;
            QueryOp::Take { count }
        }
        "offset" => {
            let count = value.parse().with_context(|| {
                format!("failed to parse offset. Expected u64, got '{}'", value)
            })?;
            QueryOp::Skip { count }
        }
        "filter" => {
            let expr = convert_json_to_filter_expr(base_type, value)
                .context("failed to convert json filter to filter expression")?;
            if let Some(expression) = expr {
                QueryOp::Filter { expression }
            } else {
                return Ok(None);
            }
        }
        _ => {
            return Ok(None);
        }
    };
    Ok(Some(op))
}

fn convert_json_to_filter_expr(base_type: &Arc<ObjectType>, value: &str) -> Result<Option<Expr>> {
    let filter =
        serde_json::from_str::<JsonObject>(value).context("failed to parse JSON filter")?;
    let mut filter_expr = None;
    for (field_name, v) in filter.iter() {
        if let Some(field) = base_type.get_field(field_name) {
            let err_msg = |ty| {
                format!(
                    "failed to convert filter value '{:?}' to {} for field '{}'",
                    v, ty, field_name
                )
            };
            let literal: Literal = match &field.type_ {
                Type::String | Type::Id => v.as_str().with_context(|| err_msg("string/id"))?.into(),
                Type::Float => v.as_f64().with_context(|| err_msg("float"))?.into(),
                Type::Boolean => v.as_bool().with_context(|| err_msg("bool"))?.into(),
                _ => anyhow::bail!(
                    "trying to filter on entity-type field '{}' which is not supported",
                    field_name
                ),
            };
            let f: Expr = BinaryExpr {
                left: Box::new(
                    PropertyAccess {
                        property: field_name.to_owned(),
                        object: Box::new(Expr::Parameter { position: 0 }),
                    }
                    .into(),
                ),
                op: BinaryOp::Eq,
                right: Box::new(literal.into()),
            }
            .into();
            if let Some(expr) = filter_expr {
                filter_expr = Some(
                    BinaryExpr {
                        left: Box::new(expr),
                        op: BinaryOp::And,
                        right: Box::new(f),
                    }
                    .into(),
                );
            } else {
                filter_expr = Some(f);
            }
        } else {
            anyhow::bail!(
                "entity '{}' doesn't have field named '{}'",
                base_type.name(),
                field_name
            );
        }
    }
    Ok(filter_expr)
}
