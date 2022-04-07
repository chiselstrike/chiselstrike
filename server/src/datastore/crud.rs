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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Field, FieldDescriptor, ObjectDescriptor};

    use itertools::Itertools;

    pub(crate) struct FakeField {
        name: &'static str,
        ty_: Type,
    }

    impl FieldDescriptor for FakeField {
        fn name(&self) -> String {
            self.name.to_string()
        }
        fn id(&self) -> Option<i32> {
            None
        }
        fn ty(&self) -> Type {
            self.ty_.clone()
        }
        fn api_version(&self) -> String {
            "whatever".to_string()
        }
    }

    pub(crate) struct FakeObject {
        name: &'static str,
    }

    impl ObjectDescriptor for FakeObject {
        fn name(&self) -> String {
            self.name.to_string()
        }
        fn id(&self) -> Option<i32> {
            None
        }
        fn backing_table(&self) -> String {
            "whatever".to_string()
        }
        fn api_version(&self) -> String {
            "whatever".to_string()
        }
    }

    fn make_field(name: &'static str, type_: Type) -> Field {
        let d = FakeField { name, ty_: type_ };
        Field::new(d, vec![], None, false, false)
    }

    fn make_obj(name: &'static str, fields: Vec<Field>) -> Arc<ObjectType> {
        let d = FakeObject { name };
        Arc::new(ObjectType::new(d, fields).unwrap())
    }

    fn url(query_string: &str) -> String {
        format!("http://xxx?{}", query_string)
    }

    #[test]
    fn test_query_str_to_ops() {
        let base_type = make_obj(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        );
        {
            let ops = query_str_to_ops(&base_type, &url("limit=2")).unwrap();
            assert!(ops.len() == 1);
            let op = ops.first().unwrap();
            assert!(matches!(op, QueryOp::Take { count: 2 }));
        }
        {
            let ops = query_str_to_ops(&base_type, &url("offset=3")).unwrap();
            assert!(ops.len() == 1);
            let op = ops.first().unwrap();
            assert!(matches!(op, QueryOp::Skip { count: 3 }));
        }
        {
            let ops1 = query_str_to_ops(&base_type, &url("sort=age")).unwrap();
            assert_eq!(
                ops1,
                vec![QueryOp::SortBy(SortBy {
                    field_name: "age".into(),
                    ascending: true
                })]
            );
            let ops2 = query_str_to_ops(&base_type, &url("sort=%2Bage")).unwrap();
            assert_eq!(ops1, ops2);
            let ops3 = query_str_to_ops(&base_type, &url("sort=-age")).unwrap();
            assert_eq!(
                ops3,
                vec![QueryOp::SortBy(SortBy {
                    field_name: "age".into(),
                    ascending: false
                })]
            );
        }
        {
            let raw_ops = vec!["limit=3", "offset=7", "sort=age"];
            for perm in raw_ops.iter().permutations(raw_ops.len()) {
                let query_string = perm.iter().join("&");
                let ops1 = query_str_to_ops(&base_type, &url(&query_string)).unwrap();
                assert_eq!(
                    ops1,
                    vec![
                        QueryOp::SortBy(SortBy {
                            field_name: "age".into(),
                            ascending: true
                        }),
                        QueryOp::Skip { count: 7 },
                        QueryOp::Take { count: 3 },
                    ]
                );
            }
        }
    }

    #[test]
    fn test_query_str_to_ops_errors() {
        let base_type = make_obj(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        );
        assert!(query_str_to_ops(&base_type, &url("limit=two")).is_err());
        assert!(query_str_to_ops(&base_type, &url("limit=true")).is_err());

        assert!(query_str_to_ops(&base_type, &url("offset=two")).is_err());
        assert!(query_str_to_ops(&base_type, &url("offset=true")).is_err());

        assert!(query_str_to_ops(&base_type, &url("sort=age1")).is_err());
        assert!(query_str_to_ops(&base_type, &url("sort=%2Bnotname")).is_err());
        assert!(query_str_to_ops(&base_type, &url("sort=-notname")).is_err());
        assert!(query_str_to_ops(&base_type, &url("sort=--age")).is_err());
        assert!(query_str_to_ops(&base_type, &url("sort=age aa")).is_err());
    }
}
