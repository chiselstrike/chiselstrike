use crate::datastore::engine::{QueryEngine, TransactionStatic};
use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, Literal, PropertyAccess};
use crate::datastore::query::{Mutation, QueryOp, QueryPlan, RequestContext, SortBy, SortKey};
use crate::types::{ObjectType, Type};
use crate::JsonObject;
use anyhow::{Context, Result};
use futures::{Future, Stream, StreamExt};
use serde_derive::{Deserialize, Serialize};
use std::sync::Arc;
use url::Url;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct QueryParams {
    #[serde(rename = "typeName")]
    type_name: String,
    url: String,
}

/// Parses CRUD `params` and runs the query with provided `query_engine` returning
/// JSON of results.
pub(crate) fn run_query(
    context: &RequestContext<'_>,
    params: QueryParams,
    query_engine: Arc<QueryEngine>,
    tr: TransactionStatic,
) -> impl Future<Output = Result<Vec<JsonObject>>> {
    let stream = make_stream(context, params, query_engine, tr);
    async {
        stream?
            .collect::<Vec<Result<JsonObject>>>()
            .await
            .into_iter()
            .collect::<Result<_>>()
            .context("failed to collect result rows from the database")
    }
}

fn make_stream(
    context: &RequestContext<'_>,
    params: QueryParams,
    query_engine: Arc<QueryEngine>,
    tr: TransactionStatic,
) -> Result<impl Stream<Item = Result<JsonObject>>> {
    let url = Url::parse(&params.url)
        .with_context(|| format!("crud endpoint failed to parse url: '{}'", params.url))?;
    let query = Query::from_url(context, &params.type_name, &url)?;
    let query_plan = QueryPlan::from_ops(context, &query.base_type, query.to_ops())?;
    query_engine.query(tr.clone(), query_plan)
}

/// Constructs Delete Mutation from CRUD url.
pub(crate) fn delete_from_url(c: &RequestContext, type_name: &str, url: &str) -> Result<Mutation> {
    let base_entity = match c.ts.lookup_type(type_name, &c.api_version) {
        Ok(Type::Object(ty)) => ty,
        Ok(ty) => anyhow::bail!("Cannot delete scalar type {type_name} ({})", ty.name()),
        Err(_) => anyhow::bail!("Cannot delete from type `{type_name}`, type not found"),
    };
    let filter_expr = url_to_filter(&base_entity, url)
        .context("failed to convert crud URL to filter expression")?;
    if filter_expr.is_none() {
        let q = Url::parse(url).with_context(|| format!("failed to parse query string '{url}'"))?;
        let delete_all = q
            .query_pairs()
            .any(|(key, value)| key == "all" && value == "true");
        if !delete_all {
            anyhow::bail!("crud delete requires a filter to be set or `all=true` parameter.")
        }
    }
    Mutation::delete_from_expr(c, type_name, &filter_expr)
}

/// Query is used in the process of parsing crud url query to rust representation.
struct Query {
    base_type: Arc<ObjectType>,
    limit: Option<u64>,
    offset: Option<u64>,
    sort: Option<SortBy>,
    /// Filters restricting the result set. They will be joined in AND-fashion.
    filters: Vec<Expr>,
}

impl Query {
    fn new(base_type: Arc<ObjectType>) -> Self {
        Query {
            base_type,
            limit: None,
            offset: None,
            sort: None,
            filters: vec![],
        }
    }

    /// Parses provided `url` and builds a `Query` that can be used to build a `QueryPlan`.
    fn from_url(c: &RequestContext, entity_name: &str, url: &Url) -> Result<Self> {
        let base_type = &c
            .ts
            .lookup_object_type(entity_name, &c.api_version)
            .context("unexpected type name as crud query base type")?;

        let mut q = Query::new(base_type.clone());
        for (param_key, value) in url.query_pairs().into_owned() {
            match param_key.as_str() {
                "sort" => q.sort = parse_sort(base_type, &value)?.into(),
                "limit" => {
                    let l = value.parse().with_context(|| {
                        format!("failed to parse {param_key}. Expected u64, got '{}'", value)
                    })?;
                    q.limit = Some(l);
                }
                "offset" => {
                    let o = value.parse().with_context(|| {
                        format!("failed to parse offset. Expected u64, got '{}'", value)
                    })?;
                    q.offset = Some(o);
                }
                _ => {
                    if let Some(param_key) = param_key.strip_prefix('.') {
                        let expr =
                            filter_from_param(base_type, param_key, &value).with_context(|| {
                                format!("failed to parse filter {param_key}={value}")
                            })?;
                        q.filters.push(expr);
                    }
                }
            }
        }
        Ok(q)
    }

    /// Makes `QueryPlan` based on the CRUD parameters that were parsed by `from_url` method.
    /// The `QueryPlan` can be used to retrieve desired results from the database.
    fn to_ops(&self) -> Vec<QueryOp> {
        let mut ops = vec![];
        if let Some(sort) = &self.sort {
            ops.push(QueryOp::SortBy(sort.clone()));
        }
        for f_expr in self.filters.iter().cloned() {
            ops.push(QueryOp::Filter { expression: f_expr });
        }
        if let Some(offset) = self.offset {
            ops.push(QueryOp::Skip { count: offset });
        }
        if let Some(limit) = self.limit {
            ops.push(QueryOp::Take { count: limit });
        }
        ops
    }
}

/// Parses all CRUD query-string filters over `base_type` from provided `url`.
fn url_to_filter(base_type: &Arc<ObjectType>, url: &str) -> Result<Option<Expr>> {
    let mut filter = None;
    let q = Url::parse(url).with_context(|| format!("failed to parse query string '{}'", url))?;
    for (param_key, value) in q.query_pairs().into_owned() {
        let param_key = param_key.to_string();
        if let Some(param_key) = param_key.strip_prefix('.') {
            let expression = filter_from_param(base_type, param_key, &value)
                .context("failed to parse filter")?;

            filter = filter
                .map_or(expression.clone(), |e| BinaryExpr::and(expression, e))
                .into();
        }
    }
    Ok(filter)
}

fn parse_sort(base_type: &Arc<ObjectType>, value: &str) -> Result<SortBy> {
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
    Ok(SortBy {
        keys: vec![SortKey {
            field_name: field_name.to_owned(),
            ascending,
        }],
    })
}

/// Constructs results filter by parsing query string's `param_key` and `value`.
fn filter_from_param(base_type: &Arc<ObjectType>, param_key: &str, value: &str) -> Result<Expr> {
    let tokens: Vec<_> = param_key.split('~').collect();
    anyhow::ensure!(
        tokens.len() <= 2,
        "expected at most one occurrence of '~' in query parameter name '{}'",
        param_key
    );
    let fields: Vec<_> = tokens[0].split('.').collect();
    let operator = tokens.get(1).copied();
    let operator = convert_operator(operator)?;

    let (property_chain, field_type) = make_property_chain(base_type, &fields)?;

    let err_msg = |ty_name| format!("failed to convert filter value '{}' to {}", value, ty_name);
    let literal = match field_type {
        Type::Object(ty) => anyhow::bail!(
            "trying to filter by property '{}' of type '{}' which is not supported",
            fields.last().unwrap(),
            ty.name()
        ),
        Type::String | Type::Id => Literal::String(value.to_owned()),
        Type::Float => Literal::F64(value.parse::<f64>().with_context(|| err_msg("f64"))?),
        Type::Boolean => Literal::Bool(value.parse::<bool>().with_context(|| err_msg("bool"))?),
    };

    Ok(BinaryExpr::new(operator, property_chain, literal.into()).into())
}

/// Converts `fields` of `base_type` into PropertyAccess expression while ensuring that
/// provided fields are, in fact, applicable to `base_type`.
fn make_property_chain(base_type: &Arc<ObjectType>, fields: &[&str]) -> Result<(Expr, Type)> {
    anyhow::ensure!(
        !fields.is_empty(),
        "cannot make property chain from no fields"
    );
    let mut property_chain = Expr::Parameter { position: 0 };
    let mut last_type = Type::Object(base_type.clone());
    for &field_str in fields {
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
    Ok((property_chain, last_type))
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
    use crate::datastore::engine::QueryEngine;
    use crate::datastore::query::tests::{
        add_row, binary, fetch_rows, make_field, make_object, make_type_system, setup_clear_db,
        VERSION,
    };
    use crate::policies::Policies;
    use crate::types::{FieldDescriptor, ObjectDescriptor, TypeSystem};
    use crate::JsonObject;

    use itertools::Itertools;
    use serde_json::json;

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

    fn url(query_string: &str) -> String {
        format!("http://xxx?{}", query_string)
    }

    lazy_static! {
        static ref PERSON_TY: Arc<ObjectType> = make_object(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        );
        static ref COMPANY_TY: Arc<ObjectType> = make_object(
            "Company",
            vec![
                make_field("name", Type::String),
                make_field("ceo", Type::Object(PERSON_TY.clone())),
            ],
        );
        static ref ENTITIES: [&'static Arc<ObjectType>; 2] = [&*PERSON_TY, &*COMPANY_TY];
        static ref TS: TypeSystem = make_type_system(&*ENTITIES);
    }

    #[test]
    fn test_parse_filter() {
        let person_type = make_object(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        );
        let base_type = make_object(
            "Company",
            vec![
                make_field("name", Type::String),
                make_field("traded", Type::Boolean),
                make_field("employee_count", Type::Float),
                make_field("ceo", Type::Object(person_type)),
            ],
        );
        let filter_expr =
            |key: &str, value: &str| filter_from_param(&base_type, key, value).unwrap();
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

    async fn run_query(
        entity_name: &str,
        url: String,
        qe: &QueryEngine,
    ) -> Result<Vec<JsonObject>> {
        let qe = Arc::new(qe.clone());
        let tr = qe.clone().start_transaction_static().await.unwrap();
        super::run_query(
            &RequestContext {
                policies: &Policies::default(),
                ts: &make_type_system(&*ENTITIES),
                api_version: VERSION.to_owned(),
                user_id: None,
                path: "".to_string(),
            },
            QueryParams {
                type_name: entity_name.to_owned(),
                url: url.to_owned(),
            },
            qe,
            tr,
        )
        .await
    }

    async fn run_query_vec(entity_name: &str, url: String, qe: &QueryEngine) -> Vec<String> {
        let r = run_query(entity_name, url, qe).await.unwrap();
        collect_names(&r)
    }

    fn collect_names(r: &[JsonObject]) -> Vec<String> {
        r.iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn test_run_query() {
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        let alex = json!({"name": "Alex", "age": json!(40f32)});
        let john = json!({"name": "John", "age": json!(20f32)});
        let steve = json!({"name": "Steve", "age": json!(29f32)});

        let (query_engine, _db_file) = setup_clear_db(&*ENTITIES).await;
        let qe = &query_engine;
        add_row(qe, &PERSON_TY, &alan).await;
        add_row(qe, &PERSON_TY, &john).await;
        add_row(qe, &PERSON_TY, &steve).await;

        let mut r = run_query_vec("Person", url(""), qe).await;
        r.sort();
        assert_eq!(r, vec!["Alan", "John", "Steve"]);

        let r = run_query_vec("Person", url("limit=2"), qe).await;
        assert_eq!(r.len(), 2);

        // Test Sorting
        let r = run_query_vec("Person", url("sort=age"), qe).await;
        assert_eq!(r, vec!["John", "Steve", "Alan"]);

        let r = run_query_vec("Person", url("sort=%2Bage"), qe).await;
        assert_eq!(r, vec!["John", "Steve", "Alan"]);

        let r = run_query_vec("Person", url("sort=-age"), qe).await;
        assert_eq!(r, vec!["Alan", "Steve", "John"]);

        // Test Offset
        let r = run_query_vec("Person", url("sort=name&offset=1"), qe).await;
        assert_eq!(r, vec!["John", "Steve"]);

        // Test filtering
        let r = run_query_vec("Person", url(".age=10"), qe).await;
        assert_eq!(r, Vec::<String>::new());

        let r = run_query_vec("Person", url(".age=29"), qe).await;
        assert_eq!(r, vec!["Steve"]);

        let r = run_query_vec("Person", url(".age~lt=29&.name~like=%25n"), qe).await;
        assert_eq!(r, vec!["John"]);

        let mut r = run_query_vec("Person", url(".name~unlike=Al%25"), qe).await;
        r.sort();
        assert_eq!(r, vec!["John", "Steve"]);

        add_row(qe, &PERSON_TY, &alex).await;

        // Test permutations of parameters
        {
            let raw_ops = vec!["limit=2", "offset=1", "sort=age"];
            for perm in raw_ops.iter().permutations(raw_ops.len()) {
                let query_string = perm.iter().join("&");
                let r = run_query_vec("Person", url(&query_string), qe).await;
                assert_eq!(
                    r,
                    vec!["Steve", "Alan"],
                    "unexpected result for query string '{query_string}'",
                );
            }

            let raw_ops = vec!["limit=2", "offset=1", "sort=name", ".age~gt=20"];
            for perm in raw_ops.iter().permutations(raw_ops.len()) {
                let query_string = perm.iter().join("&");
                let r = run_query_vec("Person", url(&query_string), qe).await;
                assert_eq!(
                    r,
                    vec!["Alex", "Steve"],
                    "unexpected result for query string '{query_string}'",
                );
            }
        }
    }

    #[tokio::test]
    async fn test_query_str_to_ops_errors() {
        let (query_engine, _db_file) = setup_clear_db(&*ENTITIES).await;
        let qe = &query_engine;

        assert!(run_query("Person", url("limit=two"), qe).await.is_err());
        assert!(run_query("Person", url("limit=true"), qe).await.is_err());

        assert!(run_query("Person", url("offset=two"), qe).await.is_err());
        assert!(run_query("Person", url("offset=true"), qe).await.is_err());

        assert!(run_query("Person", url("sort=age1"), qe).await.is_err());
        assert!(run_query("Person", url("sort=%2Bnotname"), qe)
            .await
            .is_err());
        assert!(run_query("Person", url("sort=-notname"), qe).await.is_err());
        assert!(run_query("Person", url("sort=--age"), qe).await.is_err());
        assert!(run_query("Person", url("sort=age aa"), qe).await.is_err());

        assert!(run_query("Person", url(".agex=4"), qe).await.is_err());
        assert!(run_query("Person", url("..age=4"), qe).await.is_err());
        assert!(run_query("Person", url(".age=four"), qe).await.is_err());
        assert!(run_query("Person", url(".age=true"), qe).await.is_err());
        assert!(run_query("Person", url(".age~ltt=4"), qe).await.is_err());
        assert!(run_query("Person", url(".age~neq~lt=4"), qe).await.is_err());
        assert!(run_query("Person", url(".age.nothing=4"), qe)
            .await
            .is_err());
        assert!(run_query("Person", url(".=123"), qe).await.is_err());
    }

    #[tokio::test]
    async fn test_delete_from_crud_url() {
        fn url(query_string: &str) -> String {
            format!("http://wtf?{}", query_string)
        }

        let delete_from_url = |entity_name: &str, url: &str| {
            delete_from_url(
                &RequestContext {
                    policies: &Policies::default(),
                    ts: &make_type_system(&*ENTITIES),
                    api_version: VERSION.to_owned(),
                    user_id: None,
                    path: "".to_string(),
                },
                entity_name,
                url,
            )
            .unwrap()
        };

        let john = json!({"name": "John", "age": json!(20f32)});
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &PERSON_TY, &john).await;

            let mutation = delete_from_url("Person", &url(".name=John"));
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &PERSON_TY).await.len(), 0);
        }
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &PERSON_TY, &john).await;
            add_row(&qe, &PERSON_TY, &alan).await;

            let mutation = delete_from_url("Person", &url(".age=30"));
            qe.mutate(mutation).await.unwrap();

            let rows = fetch_rows(&qe, &PERSON_TY).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["name"], "John");
        }

        let chiselstrike = json!({"name": "ChiselStrike", "ceo": john});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &COMPANY_TY, &chiselstrike).await;

            let mutation = delete_from_url("Company", &url(".ceo.name=John"));
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &COMPANY_TY).await.len(), 0);
        }
    }
}
