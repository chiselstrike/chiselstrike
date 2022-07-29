use crate::datastore::engine::{QueryEngine, TransactionStatic};
use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, PropertyAccess, Value as ExprValue};
use crate::datastore::query::{Mutation, QueryOp, QueryPlan, RequestContext, SortBy, SortKey};
use crate::types::{Entity, Type, TypeSystem};
use crate::JsonObject;
use anyhow::{Context, Result};
use deno_core::futures;
use deno_core::url::Url;
use futures::{Future, StreamExt};
use serde_derive::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct QueryParams {
    #[serde(rename = "typeName")]
    type_name: String,
    url: Url,
}

/// Parses CRUD `params` and runs the query with provided `query_engine`.
pub(crate) fn run_query(
    context: &RequestContext<'_>,
    params: QueryParams,
    query_engine: Arc<QueryEngine>,
    tr: TransactionStatic,
) -> impl Future<Output = Result<JsonObject>> {
    let fut = run_query_impl(context, params, query_engine, tr);
    async move { fut?.await }
}

fn run_query_impl(
    context: &RequestContext<'_>,
    params: QueryParams,
    query_engine: Arc<QueryEngine>,
    tr: TransactionStatic,
) -> Result<impl Future<Output = Result<JsonObject>>> {
    let host = context.headers.get("host").cloned();
    let base_type = &context
        .ts
        .lookup_entity(&params.type_name, &context.api_version)
        .context("unexpected type name as crud query base type")?;

    let query = Query::from_url(base_type, &params.url, context.ts)?;
    let ops = query.make_query_ops()?;
    let query_plan = QueryPlan::from_ops(context, base_type, ops)?;
    let stream = query_engine.query(tr.clone(), query_plan)?;

    Ok(async move {
        let mut results = stream
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .context("failed to collect result rows from the database")?;

        // When backwards cursor is specified, the sort is reversed, hence we
        // need to reverse the results to get the original ordering.
        if let Some(cursor) = &query.cursor {
            if !cursor.forward {
                results.reverse();
            }
        }

        let mut ret = JsonObject::new();

        let next_page = get_next_page(&params, &query, &host, &results)?;
        if let Some(next_page) = next_page {
            ret.insert("next_page".into(), json!(next_page));
        }
        let prev_page = get_prev_page(&params, &query, &host, &results)?;
        if let Some(prev_page) = prev_page {
            ret.insert("prev_page".into(), json!(prev_page));
        }

        ret.insert("results".into(), json!(results));
        Ok(ret)
    })
}

/// Evaluates current query circumstances and potentially generates
/// next page url if there is a potential for retrieving elements that succeed
/// current resulting elements in query's sort.
fn get_next_page(
    params: &QueryParams,
    query: &Query,
    host: &Option<String>,
    results: &[JsonObject],
) -> Result<Option<Url>> {
    get_page(params, query, host, results, true)
}

/// Evaluates current query circumstances and potentially generates
/// prev page url if there is a potential for retrieving elements that precede
/// current resulting elements in query's sort.
fn get_prev_page(
    params: &QueryParams,
    query: &Query,
    host: &Option<String>,
    results: &[JsonObject],
) -> Result<Option<Url>> {
    get_page(params, query, host, results, false)
}

fn get_page(
    params: &QueryParams,
    query: &Query,
    host: &Option<String>,
    results: &[JsonObject],
    forward: bool,
) -> Result<Option<Url>> {
    if !results.is_empty() {
        let pivot = if forward {
            results.last().unwrap()
        } else {
            results.first().unwrap()
        };
        let cursor = cursor_from_pivot(query, pivot, forward)?;
        let url = make_page_url(&params.url, host, &cursor)?;
        return Ok(Some(url));
    } else if let Some(cursor) = &query.cursor {
        if cursor.forward != forward {
            let cursor = cursor.reversed();
            let url = make_page_url(&params.url, host, &cursor)?;
            return Ok(Some(url));
        }
    }
    Ok(None)
}

fn cursor_from_pivot(query: &Query, pivot_element: &JsonObject, forward: bool) -> Result<Cursor> {
    // If cursor is available, we must use its sort keys as query's
    // sort keys are reversed for backwards paging.
    let sort_keys = if let Some(cursor) = &query.cursor {
        cursor.axes.iter().map(|a| a.key.clone()).collect()
    } else {
        query.sort.keys.clone()
    };
    assert!(!sort_keys.is_empty());

    let mut axes = vec![];
    for key in sort_keys {
        let value = pivot_element
            .get(&key.field_name)
            .cloned()
            .with_context(|| {
                format!("failed to create cursor axis for field `{}", key.field_name)
            })?;
        axes.push(CursorAxis { key, value });
    }
    let cursor = Cursor::new(axes, forward);
    Ok(cursor)
}

/// Generates URL that can be used to retrieve previous/next page.
/// It does this by modifying the current `url`, potentially replacing
/// host address with `host` and generating url based on the current cursor.
fn make_page_url(url: &Url, host: &Option<String>, cursor: &Cursor) -> Result<Url> {
    let cursor = cursor.to_string()?;

    let mut page_url = replace_host_address(url.clone(), host)?;
    page_url.set_query(Some(""));
    for (key, value) in url.query_pairs() {
        if key == "cursor" || key == "sort" {
            continue;
        }
        page_url.query_pairs_mut().append_pair(&key, &value);
    }
    page_url.query_pairs_mut().append_pair("cursor", &cursor);

    Ok(page_url)
}

/// Constructs Delete Mutation from CRUD url.
pub(crate) fn delete_from_url(c: &RequestContext, type_name: &str, url: &str) -> Result<Mutation> {
    let base_entity = match c.ts.lookup_type(type_name, &c.api_version) {
        Ok(Type::Entity(ty)) => ty,
        Ok(ty) => anyhow::bail!("Cannot delete scalar type {type_name} ({})", ty.name()),
        Err(_) => anyhow::bail!("Cannot delete from type `{type_name}`, type not found"),
    };
    let filter_expr = url_to_filter(&base_entity, url, c.ts)
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
    page_size: u64,
    offset: Option<u64>,
    cursor: Option<Cursor>,
    sort: SortBy,
    /// Filters restricting the result set. They will be joined in AND-fashion.
    filters: Vec<Expr>,
}

impl Query {
    fn new() -> Self {
        Query {
            page_size: 1000,
            offset: None,
            cursor: None,
            sort: SortBy {
                keys: vec![SortKey {
                    field_name: "id".into(),
                    ascending: true,
                }],
            },
            filters: vec![],
        }
    }

    /// Parses provided `url` and builds a `Query` that can be used to build a `QueryPlan`.
    fn from_url(base_type: &Entity, url: &Url, ts: &TypeSystem) -> Result<Self> {
        let mut q = Query::new();
        for (param_key, value) in url.query_pairs().into_owned() {
            match param_key.as_str() {
                "sort" => q.sort = parse_sort(base_type, &value)?,
                "limit" | "page_size" => {
                    q.page_size = value.parse().with_context(|| {
                        format!("failed to parse {param_key}. Expected u64, got '{}'", value)
                    })?;
                }
                "offset" => {
                    let o = value.parse().with_context(|| {
                        format!("failed to parse offset. Expected u64, got '{}'", value)
                    })?;
                    q.offset = Some(o);
                }
                "cursor" => {
                    anyhow::ensure!(
                        q.cursor.is_none(),
                        "only one occurrence of cursor is allowed."
                    );
                    let cursor = Cursor::from_string(&value)?;
                    q.cursor = Some(cursor);
                }
                _ => {
                    if let Some(param_key) = param_key.strip_prefix('.') {
                        let expr = filter_from_param(base_type, param_key, &value, ts)
                            .with_context(|| {
                                format!("failed to parse filter {param_key}={value}")
                            })?;
                        q.filters.push(expr);
                    }
                }
            }
        }
        // We need to ensure sorting by ID for cursors to work.
        ensure_sort_by_id(&mut q.sort);
        if let Some(cursor) = &q.cursor {
            q.sort = cursor.get_sort();
            q.filters.push(cursor.get_filter(base_type, ts)?);
        }
        Ok(q)
    }

    /// Makes query ops based on the CRUD parameters that were parsed by `from_url` method.
    /// The query ops can be used to retrieve desired results from the database.
    fn make_query_ops(&self) -> Result<Vec<QueryOp>> {
        let mut ops = vec![QueryOp::SortBy(self.sort.clone())];
        for f_expr in self.filters.iter().cloned() {
            ops.push(QueryOp::Filter { expression: f_expr });
        }
        if let Some(offset) = self.offset {
            ops.push(QueryOp::Skip { count: offset });
        }
        ops.push(QueryOp::Take {
            count: self.page_size,
        });
        Ok(ops)
    }
}

fn ensure_sort_by_id(sort: &mut SortBy) {
    if !sort.keys.iter().any(|k| k.field_name == "id") {
        sort.keys.push(SortKey {
            field_name: "id".into(),
            ascending: true,
        });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cursor {
    axes: Vec<CursorAxis>,
    forward: bool,
    inclusive: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CursorAxis {
    key: SortKey,
    value: serde_json::Value,
}

impl Cursor {
    fn new(axes: Vec<CursorAxis>, forward: bool) -> Self {
        assert!(!axes.is_empty());
        Self {
            axes,
            forward,
            inclusive: false,
        }
    }

    /// Parses Cursor from base64 encoded JSON.
    fn from_string(cursor_str: &str) -> Result<Self> {
        let cursor_json = base64::decode(cursor_str)
            .context("Failed to decode cursor from base64 encoded string")?;
        let cursor: Cursor =
            serde_json::from_slice(&cursor_json).context("failed to deserialize cursor's axes")?;
        anyhow::ensure!(!cursor.axes.is_empty(), "cursor must have some sort axes");
        Ok(cursor)
    }

    /// Serializes cursor to base64 encoded JSON.
    fn to_string(&self) -> Result<String> {
        let cursor = serde_json::to_string(&self).context("failed to serialize cursor to json")?;
        Ok(base64::encode(cursor))
    }

    fn reversed(&self) -> Self {
        Self {
            axes: self.axes.clone(),
            forward: !self.forward,
            inclusive: !self.inclusive,
        }
    }

    fn get_sort(&self) -> SortBy {
        SortBy {
            keys: self
                .axes
                .iter()
                .map(|axis| {
                    let mut key = axis.key.clone();
                    key.ascending = key.ascending == self.forward;
                    key
                })
                .collect(),
        }
    }

    /// The crux of this function is using the sort axes (each axis represents one dimension
    /// in lexicographical sort) to create a filter that filters for entries that are after
    /// the last element in the given sort.
    fn get_filter(&self, base_type: &Entity, ts: &TypeSystem) -> Result<Expr> {
        let mut cmp_pairs: Vec<(Expr, BinaryOp, Expr)> = vec![];
        for axis in &self.axes {
            let op = if axis.key.ascending == self.forward {
                if self.inclusive {
                    BinaryOp::GtEq
                } else {
                    BinaryOp::Gt
                }
            } else if self.inclusive {
                BinaryOp::LtEq
            } else {
                BinaryOp::Lt
            };
            let (property_chain, field_type) =
                make_property_chain(base_type, &[&axis.key.field_name], ts)?;
            let value = json_to_value(&field_type, &axis.value)
                .context("Failed to convert axis JSON to expression value")?;

            cmp_pairs.push((property_chain, op, value));
        }
        let mut expr = None;
        for (i, (lhs, op, rhs)) in cmp_pairs.iter().enumerate() {
            let mut e: Expr = BinaryExpr::new(op.clone(), lhs.clone(), rhs.clone()).into();
            expr = expr
                .map_or(e.clone(), |expr| {
                    for (lhs, _, rhs) in &cmp_pairs[0..i] {
                        let eq = BinaryExpr::eq(lhs.clone(), rhs.clone());
                        e = BinaryExpr::and(eq, e);
                    }
                    BinaryExpr::or(expr, e)
                })
                .into();
        }
        Ok(expr.unwrap())
    }
}

fn json_to_value(field_type: &Type, value: &serde_json::Value) -> Result<Expr> {
    macro_rules! convert {
        ($as_type:ident, $ty_name:literal) => {{
            value
                .$as_type()
                .with_context(|| {
                    format!("failed to convert filter value '{}' to {}", value, $ty_name)
                })?
                .to_owned()
        }};
    }
    let expr_val = match field_type {
        Type::Entity(_) | Type::List(_) => anyhow::bail!(
            "trying to filter by property of type '{}' which is not supported",
            field_type.name()
        ),
        Type::String => ExprValue::String(convert!(as_str, "string")),
        Type::Float => ExprValue::F64(convert!(as_f64, "float")),
        Type::Boolean => ExprValue::Bool(convert!(as_bool, "bool")),
    };
    Ok(expr_val.into())
}

/// Parses all CRUD query-string filters over `base_type` from provided `url`.
fn url_to_filter(base_type: &Entity, url: &str, ts: &TypeSystem) -> Result<Option<Expr>> {
    let mut filter = None;
    let q = Url::parse(url).with_context(|| format!("failed to parse query string '{}'", url))?;
    for (param_key, value) in q.query_pairs().into_owned() {
        let param_key = param_key.to_string();
        if let Some(param_key) = param_key.strip_prefix('.') {
            let expression = filter_from_param(base_type, param_key, &value, ts)
                .context("failed to parse filter")?;

            filter = filter
                .map_or(expression.clone(), |e| BinaryExpr::and(expression, e))
                .into();
        }
    }
    Ok(filter)
}

fn parse_sort(base_type: &Entity, value: &str) -> Result<SortBy> {
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
fn filter_from_param(
    base_type: &Entity,
    param_key: &str,
    value: &str,
    ts: &TypeSystem,
) -> Result<Expr> {
    let tokens: Vec<_> = param_key.split('~').collect();
    anyhow::ensure!(
        tokens.len() <= 2,
        "expected at most one occurrence of '~' in query parameter name '{}'",
        param_key
    );
    let fields: Vec<_> = tokens[0].split('.').collect();
    let operator = tokens.get(1).copied();
    let operator = convert_operator(operator)?;

    let (property_chain, field_type) = make_property_chain(base_type, &fields, ts)?;

    let err_msg = |ty_name| format!("failed to convert filter value '{}' to {}", value, ty_name);
    let expr_value = match field_type {
        Type::Entity(ty) => anyhow::bail!(
            "trying to filter by property '{}' of type '{}' which is not supported",
            fields.last().unwrap(),
            ty.name()
        ),
        Type::String => ExprValue::String(value.to_owned()),
        Type::Float => ExprValue::F64(value.parse::<f64>().with_context(|| err_msg("f64"))?),
        Type::Boolean => ExprValue::Bool(value.parse::<bool>().with_context(|| err_msg("bool"))?),
        Type::List(_) => anyhow::bail!(
            "trying to filter by a property '{}' which is of type List",
            fields.last().unwrap()
        ),
    };

    Ok(BinaryExpr::new(operator, property_chain, expr_value.into()).into())
}

/// Converts `fields` of `base_type` into PropertyAccess expression while ensuring that
/// provided fields are, in fact, applicable to `base_type`.
fn make_property_chain(
    base_type: &Entity,
    fields: &[&str],
    ts: &TypeSystem,
) -> Result<(Expr, Type)> {
    anyhow::ensure!(
        !fields.is_empty(),
        "cannot make property chain from no fields"
    );
    let mut property_chain = Expr::Parameter { position: 0 };
    let mut last_type: Type = base_type.clone().into();
    for &field_str in fields {
        if let Type::Entity(entity) = last_type {
            if let Some(field) = entity.get_field(field_str) {
                last_type = ts.get(&field.type_id)?;
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

fn replace_host_address(mut url: Url, host_address: &Option<String>) -> Result<Url> {
    if let Some(host_address) = host_address {
        let address_tokens: Vec<_> = host_address.split(':').collect();
        anyhow::ensure!(
            address_tokens.len() <= 2,
            "unexpected number of tokens in host address"
        );
        let host = address_tokens.get(0).copied();
        let port: Option<u16> = address_tokens
            .get(1)
            .map(|p| p.parse())
            .transpose()
            .context("Failed to parse address port")?;
        url.set_host(host)?;
        url.set_port(port)
            .map_err(|_| anyhow::anyhow!("Failed to set url port"))?;
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datastore::engine::QueryEngine;
    use crate::datastore::query::tests::{
        add_row, binary, fetch_rows, make_entity, make_field, make_type_system, setup_clear_db,
        VERSION,
    };
    use crate::policies::Policies;
    use crate::types::{FieldDescriptor, ObjectDescriptor};
    use crate::JsonObject;

    use itertools::Itertools;
    use once_cell::sync::Lazy;
    use serde_json::json;
    use std::collections::HashMap;

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
        fn junction_table(&self) -> Option<String> {
            None
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

    fn url(query_string: &str) -> Url {
        Url::parse(&format!("http://xxx?{}", query_string)).unwrap()
    }

    static PERSON_TY: Lazy<Entity> = Lazy::new(|| {
        make_entity(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        )
    });
    static COMPANY_TY: Lazy<Entity> = Lazy::new(|| {
        make_entity(
            "Company",
            vec![
                make_field("name", Type::String),
                make_field("ceo", PERSON_TY.clone().into()),
            ],
        )
    });
    static ENTITIES: Lazy<[Entity; 2]> = Lazy::new(|| [PERSON_TY.clone(), COMPANY_TY.clone()]);
    static TYPE_SYSTEM: Lazy<TypeSystem> = Lazy::new(|| make_type_system(&*ENTITIES));

    #[test]
    fn test_replace_host_address() {
        fn check_replace(url: &str, host_address: &str, expected_url: &str) {
            let url = Url::parse(url).unwrap();
            let new_url = replace_host_address(url, &Some(host_address.to_owned())).unwrap();
            assert_eq!(new_url.as_str(), expected_url);
        }

        check_replace(
            "http://example.com/foo",
            "example.com:999",
            "http://example.com:999/foo",
        );
        check_replace(
            "http://example.com:777/foo",
            "example.com:999",
            "http://example.com:999/foo",
        );
        check_replace(
            "http://example.com/foo",
            "example.com:999",
            "http://example.com:999/foo",
        );
        check_replace(
            "http://192.168.1.1:777/foo",
            "example.com:999",
            "http://example.com:999/foo",
        );
        check_replace(
            "http://192.168.1.1:777/foo",
            "example.com",
            "http://example.com/foo",
        );
        check_replace(
            "http://example.com:777/foo",
            "192.168.1.1:999",
            "http://192.168.1.1:999/foo",
        );
        check_replace(
            "http://example.com:777/foo",
            "192.168.1.1",
            "http://192.168.1.1/foo",
        );
    }

    #[test]
    fn test_parse_filter() {
        let person_type = make_entity(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        );
        let base_type = make_entity(
            "Company",
            vec![
                make_field("name", Type::String),
                make_field("traded", Type::Boolean),
                make_field("employee_count", Type::Float),
                make_field("ceo", person_type.into()),
            ],
        );
        let filter_expr = |key: &str, value: &str| {
            filter_from_param(&base_type, key, value, &TYPE_SYSTEM).unwrap()
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

    async fn run_query(entity_name: &str, url: Url, qe: &QueryEngine) -> Result<JsonObject> {
        run_query_with_headers(entity_name, url, qe, HashMap::default()).await
    }

    async fn run_query_with_headers(
        entity_name: &str,
        url: Url,
        qe: &QueryEngine,
        headers: HashMap<String, String>,
    ) -> Result<JsonObject> {
        let qe = Arc::new(qe.clone());
        let tr = qe.clone().start_transaction_static().await.unwrap();
        super::run_query(
            &RequestContext {
                policies: &Policies::default(),
                ts: &make_type_system(&*ENTITIES),
                api_version: VERSION.to_owned(),
                user_id: None,
                path: "".to_string(),
                headers,
            },
            QueryParams {
                type_name: entity_name.to_owned(),
                url,
            },
            qe,
            tr,
        )
        .await
    }

    async fn run_query_vec(entity_name: &str, url: Url, qe: &QueryEngine) -> Vec<String> {
        let r = run_query(entity_name, url, qe).await.unwrap();
        collect_names(&r)
    }

    fn collect_names(r: &JsonObject) -> Vec<String> {
        r["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn test_run_query() {
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        let john = json!({"name": "John", "age": json!(20f32)});
        let steve = json!({"name": "Steve", "age": json!(29f32)});

        let (query_engine, _db_file) = setup_clear_db(&*ENTITIES).await;
        let qe = &query_engine;
        add_row(qe, &PERSON_TY, &alan, &TYPE_SYSTEM).await;
        add_row(qe, &PERSON_TY, &john, &TYPE_SYSTEM).await;
        add_row(qe, &PERSON_TY, &steve, &TYPE_SYSTEM).await;

        let mut r = run_query_vec("Person", url(""), qe).await;
        r.sort();
        assert_eq!(r, vec!["Alan", "John", "Steve"]);

        let r = run_query_vec("Person", url("limit=2"), qe).await;
        assert_eq!(r.len(), 2);

        let r = run_query_vec("Person", url("page_size=2"), qe).await;
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

        let alex = json!({"name": "Alex", "age": json!(40f32)});
        add_row(qe, &PERSON_TY, &alex, &TYPE_SYSTEM).await;
        // Test permutations of parameters
        {
            let raw_ops = vec!["page_size=2", "offset=1", "sort=age"];
            for perm in raw_ops.iter().permutations(raw_ops.len()) {
                let query_string = perm.iter().join("&");
                let r = run_query_vec("Person", url(&query_string), qe).await;
                assert_eq!(
                    r,
                    vec!["Steve", "Alan"],
                    "unexpected result for query string '{query_string}'",
                );
            }

            let raw_ops = vec!["page_size=2", "offset=1", "sort=name", ".age~gt=20"];
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

        // Test multiple levels of indirection
        let chiselstrike = json!({"name": "ChiselStrike", "ceo": john});
        let thunderstrike = json!({"name": "ThunderStrike", "ceo": alan});
        {
            add_row(qe, &COMPANY_TY, &chiselstrike, &TYPE_SYSTEM).await;
            add_row(qe, &COMPANY_TY, &thunderstrike, &TYPE_SYSTEM).await;

            let r = run_query_vec("Company", url(".ceo.age~lte=20"), qe).await;
            assert_eq!(r, vec!["ChiselStrike"]);
        }
    }

    #[tokio::test]
    async fn test_paging() {
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        let alex = json!({"name": "Alex", "age": json!(40f32)});
        let john = json!({"name": "John", "age": json!(20f32)});
        let steve = json!({"name": "Steve", "age": json!(29f32)});

        let (query_engine, _db_file) = setup_clear_db(&*ENTITIES).await;
        let qe = &query_engine;
        add_row(qe, &PERSON_TY, &alan, &TYPE_SYSTEM).await;
        add_row(qe, &PERSON_TY, &john, &TYPE_SYSTEM).await;
        add_row(qe, &PERSON_TY, &steve, &TYPE_SYSTEM).await;
        add_row(qe, &PERSON_TY, &alex, &TYPE_SYSTEM).await;

        fn get_url(raw: &serde_json::Value) -> Url {
            Url::parse(raw.as_str().unwrap()).unwrap()
        }

        // Simple forward step.
        {
            let r = run_query("Person", url("sort=name&page_size=2"), qe)
                .await
                .unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan", "Alex"]);

            assert!(r.contains_key("next_page"));
            let next_page = get_url(&r["next_page"]);
            let r = run_query("Person", next_page, qe).await.unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["John", "Steve"]);
        }

        // Empty pre-first page loop
        {
            let r = run_query("Person", url("sort=name&page_size=1"), qe)
                .await
                .unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan"]);

            assert!(r.contains_key("prev_page"));
            let prev_page = get_url(&r["prev_page"]);

            let r = run_query("Person", prev_page.clone(), qe).await.unwrap();
            let names = collect_names(&r);
            assert!(names.is_empty());

            assert!(r.contains_key("next_page"));
            let first_page = get_url(&r["next_page"]);
            let r = run_query("Person", first_page.clone(), qe).await.unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan"]);
        }

        // Empty last page loop
        {
            let r = run_query("Person", url("sort=name&page_size=4"), qe)
                .await
                .unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan", "Alex", "John", "Steve"]);

            assert!(r.contains_key("next_page"));
            let next_page = get_url(&r["next_page"]);

            let r = run_query("Person", next_page.clone(), qe).await.unwrap();
            let names = collect_names(&r);
            assert!(names.is_empty());

            assert!(r.contains_key("prev_page"));
            let last_page = get_url(&r["prev_page"]);
            let r = run_query("Person", last_page.clone(), qe).await.unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan", "Alex", "John", "Steve"]);
        }

        async fn run_cursor_test(
            qe: &QueryEngine,
            mut page_url: Url,
            n_steps: usize,
            page_size: usize,
        ) -> Vec<String> {
            let mut all_names = vec![];
            for i in 0..n_steps {
                let r = run_query("Person", page_url.clone(), qe).await.unwrap();

                // Check backward cursors
                if i == 0 {
                    assert!(r.contains_key("prev_page"));
                } else if i != 0 {
                    // Check that previous page returns the same results as
                    // before using next_page.
                    let prev_page = get_url(&r["prev_page"]);
                    let r = run_query("Person", prev_page.clone(), qe).await.unwrap();
                    let prev_names = collect_names(&r);
                    assert_eq!(prev_names.len(), page_size);
                    assert_eq!(prev_names, all_names[all_names.len() - page_size..]);

                    // Check that a sequence page1 -> page["prev_page"] (page2) -> page2["next_page"] (page3)
                    // is a cycle, i.e. page1 == page3.
                    let loopback_page = get_url(&r["next_page"]);
                    assert_eq!(page_url, loopback_page);

                    if i == 1 {
                        // We go from the second page to first and beyond to an empty page.
                        let prev_page = get_url(&r["prev_page"]);
                        let r = run_query("Person", prev_page.clone(), qe).await.unwrap();
                        assert!(r.contains_key("next_page"));
                    }
                }

                let names = collect_names(&r);
                all_names.extend(names.clone());
                // Check forward cursors
                if i == n_steps - 1 {
                    assert!(names.is_empty());
                    assert!(!r.contains_key("next_page"));
                } else {
                    assert_eq!(names.len(), page_size);
                    page_url = get_url(&r["next_page"]);
                }
            }
            all_names
        }
        {
            let mut all_names = run_cursor_test(qe, url("page_size=1"), 5, 1).await;
            all_names.sort();
            assert_eq!(all_names, vec!["Alan", "Alex", "John", "Steve"]);
        }
        {
            let all_names = run_cursor_test(qe, url("sort=name&page_size=2"), 3, 2).await;
            assert_eq!(all_names, vec!["Alan", "Alex", "John", "Steve"]);
        }
        {
            let r = run_query("Person", url("sort=name&page_size=5"), qe)
                .await
                .unwrap();

            assert!(r.contains_key("next_page"));
            assert!(r.contains_key("prev_page"));
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan", "Alex", "John", "Steve"]);
        }
        // Check HOST header handling.
        {
            let headers = HashMap::<String, String>::from_iter([(
                "host".to_string(),
                "myhost.com:666".to_string(),
            )]);
            let r = run_query_with_headers("Person", url("page_size=1"), qe, headers)
                .await
                .unwrap();
            let next_page = r["next_page"].as_str().unwrap();
            assert!(next_page.starts_with("http://myhost.com:666"));
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
                    headers: HashMap::default(),
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
            add_row(&qe, &PERSON_TY, &john, &TYPE_SYSTEM).await;

            let mutation = delete_from_url("Person", &url(".name=John"));
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &PERSON_TY).await.len(), 0);
        }
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &PERSON_TY, &john, &TYPE_SYSTEM).await;
            add_row(&qe, &PERSON_TY, &alan, &TYPE_SYSTEM).await;

            let mutation = delete_from_url("Person", &url(".age=30"));
            qe.mutate(mutation).await.unwrap();

            let rows = fetch_rows(&qe, &PERSON_TY).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["name"], "John");
        }

        let chiselstrike = json!({"name": "ChiselStrike", "ceo": john});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &COMPANY_TY, &chiselstrike, &TYPE_SYSTEM).await;

            let mutation = delete_from_url("Company", &url(".ceo.name=John"));
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &COMPANY_TY).await.len(), 0);
        }
    }
}
