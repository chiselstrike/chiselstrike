use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, Literal, PropertyAccess};
use crate::datastore::query::{QueryOp, SortBy};
use crate::types::{ObjectType, Type};

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
        match op {
            Some(QueryOp::Skip { .. }) => offset = op,
            Some(QueryOp::Take { .. }) => limit = op,
            Some(op) => ops.push(op),
            _ => {}
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
        _ => {
            if let Some(param_key) = param_key.strip_prefix('.') {
                parse_filter(base_type, param_key, value).context("failed to parse filter")?
            } else {
                return Ok(None);
            }
        }
    };
    Ok(Some(op))
}

fn parse_filter(base_type: &Arc<ObjectType>, param_key: &str, value: &str) -> Result<QueryOp> {
    let tokens: Vec<_> = param_key.split('~').collect();
    anyhow::ensure!(
        tokens.len() <= 2,
        "expected at most one occurrence of '~' in query parameter name '{}'",
        param_key
    );
    let fields: Vec<_> = tokens[0].split('.').collect();
    let operator = tokens.get(1).copied();

    let mut property_chain = Expr::Parameter { position: 0 };
    let mut last_type = Type::Object(base_type.clone());
    for &field_str in &fields {
        if let Type::Object(entity) = last_type {
            if let Some(field) = entity.get_field(field_str) {
                last_type = field.type_.clone();
            } else {
                anyhow::bail!(
                    "entity '{}' doesn't have field '{}'",
                    entity.name(),
                    field_str
                );
            }
        } else {
            anyhow::bail!(
                "invalid property access: no field '{}' on type '{}'",
                field_str,
                last_type.name()
            );
        }
        property_chain = Expr::Property(PropertyAccess {
            property: field_str.to_owned(),
            object: Box::new(property_chain),
        });
    }

    let err_msg = |ty_name| format!("failed to convert filter value '{}' to {}", value, ty_name);
    let literal = match last_type {
        Type::Object(ty) => anyhow::bail!(
            "trying to filter by property '{}' of type '{}' which is not supported",
            fields.last().unwrap(),
            ty.name()
        ),
        Type::String | Type::Id => Literal::String(value.to_owned()),
        Type::Float => Literal::F64(value.parse::<f64>().with_context(|| err_msg("f64"))?),
        Type::Boolean => Literal::Bool(value.parse::<bool>().with_context(|| err_msg("bool"))?),
    };

    let op = QueryOp::Filter {
        expression: Expr::Binary(BinaryExpr {
            left: Box::new(property_chain),
            op: convert_operator(operator)?,
            right: Box::new(literal.into()),
        }),
    };
    Ok(op)
}

fn convert_operator(op_str: Option<&str>) -> Result<BinaryOp> {
    if op_str == None {
        return Ok(BinaryOp::Eq);
    }
    let op = match op_str.unwrap() {
        "ne" => BinaryOp::NotEq,
        "lt" => BinaryOp::Lt,
        "lte" => BinaryOp::LtEq,
        "gt" => BinaryOp::Gt,
        "gte" => BinaryOp::GtEq,
        "like" => BinaryOp::Like,
        "unlike" => BinaryOp::NotLike,
        op => anyhow::bail!("found unsupported operator '{}'", op),
    };
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Field, FieldDescriptor, ObjectDescriptor};

    use itertools::Itertools;

    pub(crate) struct FakeField {
        name: &'static str,
        ty: Type,
    }

    impl FieldDescriptor for FakeField {
        fn name(&self) -> String {
            self.name.to_string()
        }
        fn id(&self) -> Option<i32> {
            None
        }
        fn ty(&self) -> Type {
            self.ty.clone()
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
        let d = FakeField { name, ty: type_ };
        Field::new(d, vec![], None, false, false)
    }

    fn make_obj(name: &'static str, fields: Vec<Field>) -> Arc<ObjectType> {
        let d = FakeObject { name };
        Arc::new(ObjectType::new(d, fields).unwrap())
    }

    fn url(query_string: &str) -> String {
        format!("http://xxx?{}", query_string)
    }

    fn binary(fields: &[&'static str], op: BinaryOp, literal: Literal) -> Expr {
        assert!(!fields.len() > 0);
        let mut field_chain = Expr::Parameter { position: 0 };
        for field_name in fields {
            field_chain = PropertyAccess {
                property: field_name.to_string(),
                object: field_chain.into(),
            }
            .into();
        }
        BinaryExpr {
            left: Box::new(field_chain),
            op,
            right: Box::new(literal.into()),
        }
        .into()
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
            let ops = query_str_to_ops(&base_type, &url(".age=10")).unwrap();
            assert_eq!(
                ops,
                vec![QueryOp::Filter {
                    expression: binary(&["age"], BinaryOp::Eq, (10.).into())
                }]
            );

            let ops =
                query_str_to_ops(&base_type, &url(".age~lte=10&.name~unlike=foo%25")).unwrap();
            assert_eq!(
                ops,
                vec![
                    QueryOp::Filter {
                        expression: binary(&["age"], BinaryOp::LtEq, (10.).into())
                    },
                    QueryOp::Filter {
                        expression: binary(&["name"], BinaryOp::NotLike, "foo%".into())
                    }
                ]
            );
        }
        {
            let raw_ops = vec!["limit=3", "offset=7", "sort=age"];
            for perm in raw_ops.iter().permutations(raw_ops.len()) {
                let query_string = perm.iter().join("&");
                let ops = query_str_to_ops(&base_type, &url(&query_string)).unwrap();

                assert_eq!(
                    ops,
                    vec![
                        QueryOp::SortBy(SortBy {
                            field_name: "age".into(),
                            ascending: true
                        }),
                        QueryOp::Skip { count: 7 },
                        QueryOp::Take { count: 3 },
                    ],
                    "unexpected ops for query string '{}'",
                    query_string
                );
            }
        }
        {
            let raw_ops = vec!["limit=3", "offset=7", "sort=age", ".age~gte=10"];
            for perm in raw_ops.iter().permutations(raw_ops.len()) {
                let query_string = perm.iter().join("&");
                let ops1 = query_str_to_ops(&base_type, &url(&query_string)).unwrap();

                assert!(
                    ops1.len() == 4,
                    "unexpected ops length, query string: {}",
                    query_string
                );
                assert!(
                    ops1.iter().any(|op| op.as_sort_by().is_some()),
                    "ops don't contain sort, query string: {}",
                    query_string
                );
                assert!(
                    ops1.iter().any(|op| op.as_filter().is_some()),
                    "ops don't contain filter, query string: {}",
                    query_string
                );
                assert_eq!(
                    &ops1[ops1.len() - 2..],
                    vec![QueryOp::Skip { count: 7 }, QueryOp::Take { count: 3 },],
                    "unexpected two ops for query string '{}'",
                    query_string
                );
            }
        }
    }

    #[test]
    fn test_parse_filter() {
        let person_type = make_obj(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        );
        let base_type = make_obj(
            "Company",
            vec![
                make_field("name", Type::String),
                make_field("traded", Type::Boolean),
                make_field("employee_count", Type::Float),
                make_field("ceo", Type::Object(person_type)),
            ],
        );
        let filter_expr = |key: &str, value: &str| {
            parse_filter(&base_type, key, value)
                .unwrap()
                .as_filter()
                .unwrap()
                .clone()
        };
        let ops = [
            (BinaryOp::Eq, ""),
            (BinaryOp::NotEq, "~ne"),
            (BinaryOp::Lt, "~lt"),
            (BinaryOp::LtEq, "~lte"),
            (BinaryOp::Gt, "~gt"),
            (BinaryOp::GtEq, "~gte"),
        ];
        {
            for (op, op_str) in &ops {
                let key = &format!("employee_count{}", op_str);
                assert_eq!(
                    filter_expr(key, "10"),
                    binary(&["employee_count"], op.clone(), (10.).into()),
                    "unexpected filter for filter key '{}'",
                    key
                );
            }

            for (op, op_str) in &ops {
                let key = &format!("traded{}", op_str);
                assert_eq!(
                    filter_expr(key, "true"),
                    binary(&["traded"], op.clone(), (true).into()),
                    "unexpected filter for filter key '{}'",
                    key
                );
            }
        }
        {
            for (op, op_str) in ops {
                let key = &format!("name{}", op_str);
                assert_eq!(
                    filter_expr(key, "YourNameHere inc."),
                    binary(&["name"], op, "YourNameHere inc.".into()),
                    "unexpected filter for filter key '{}'",
                    key
                );
            }
        }
        {
            assert_eq!(
                filter_expr("ceo.name~ne", "Rudolf"),
                binary(&["ceo", "name"], BinaryOp::NotEq, "Rudolf".into()),
            );
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

        assert!(query_str_to_ops(&base_type, &url(".agex=4")).is_err());
        assert!(query_str_to_ops(&base_type, &url("..age=4")).is_err());
        assert!(query_str_to_ops(&base_type, &url(".age=four")).is_err());
        assert!(query_str_to_ops(&base_type, &url(".age=true")).is_err());
        assert!(query_str_to_ops(&base_type, &url(".age~ltt=4")).is_err());
        assert!(query_str_to_ops(&base_type, &url(".age~neq~lt=4")).is_err());
        assert!(query_str_to_ops(&base_type, &url(".age.nothing=4")).is_err());
        assert!(query_str_to_ops(&base_type, &url(".=123")).is_err());
    }
}
