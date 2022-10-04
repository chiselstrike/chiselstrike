// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::pin::Pin;

use anyhow::{Context, Result};
use deno_core::futures;
use futures::{Future, Stream, StreamExt};
use guard::guard;
use serde_derive::{Deserialize, Serialize};
use serde_json::json;

use super::query::{Mutation, QueryOp, QueryPlan, SortBy, SortKey};
use super::value::EntityMap;
use super::{DataContext, QueryEngine};
use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, PropertyAccess, Value as ExprValue};
use crate::policy::{PolicyProcessor, ValidatedEntityStream};
use crate::types::{Entity, Type, TypeSystem};
use crate::{JsonObject, FEATURES};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryParams {
    pub(super) type_name: String,
    pub(super) url_path: String,
    pub(super) url_query: Vec<(String, String)>,
}

impl QueryEngine {
    /// Parses CRUD `params` and runs the query with provided `query_engine`.
    pub fn run_query(
        &self,
        ctx: &DataContext,
        params: QueryParams,
    ) -> impl Future<Output = Result<JsonObject>> + '_ {
        let fut = self.run_query_impl(ctx, params);
        async move { fut?.await }
    }

    fn run_query_impl(
        &self,
        ctx: &DataContext,
        params: QueryParams,
    ) -> Result<impl Future<Output = Result<JsonObject>> + '_> {
        let base_type = &ctx
            .type_system
            .lookup_entity(&params.type_name)
            .context("unexpected type name as crud query base type")?;

        let query = Query::from_url_query(base_type, &params.url_query, &ctx.type_system)?;
        let ops = query.make_query_ops()?;
        let query_plan = QueryPlan::from_ops(ctx, base_type, ops)?;
        let stream = self.query(ctx.txn.clone(), query_plan)?;

        let stream: Pin<Box<dyn Stream<Item = _>>> = if FEATURES.typescript_policies {
            let validator = PolicyProcessor {
                ty: base_type.object_type().clone(),
                ctx: ctx.policy_context.clone(),
            };
            Box::pin(ValidatedEntityStream { stream, validator })
        } else {
            Box::pin(stream)
        };

        Ok(async move {
            let results = stream
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>>>();

            let mut results = match results {
                Ok(res) => res,
                Err(_) => results.context("failed to collect result rows from the database")?,
            };

            // When backwards cursor is specified, the sort is reversed, hence we
            // need to reverse the results to get the original ordering.
            if let Some(cursor) = &query.cursor {
                if !cursor.forward {
                    results.reverse();
                }
            }
            let results: Vec<_> = results
            .iter()
            .map(|entity_fields: &EntityMap| {
                let v = serde_json::to_value(entity_fields).unwrap();
                guard! {let serde_json::Value::Object(map) = v else { panic!("expected json object") }}
                map
            })
            .collect();

            let mut ret = JsonObject::new();
            let next_page = get_next_page(&params, &query, &results)?;
            if let Some(next_page) = next_page {
                ret.insert("next_page".into(), json!(next_page));
            }
            let prev_page = get_prev_page(&params, &query, &results)?;
            if let Some(prev_page) = prev_page {
                ret.insert("prev_page".into(), json!(prev_page));
            }

            ret.insert("results".into(), json!(results));
            Ok(ret)
        })
    }
}

/// Evaluates current query circumstances and potentially generates
/// next page url if there is a potential for retrieving elements that succeed
/// current resulting elements in query's sort.
fn get_next_page(
    params: &QueryParams,
    query: &Query,
    results: &[JsonObject],
) -> Result<Option<String>> {
    get_page(params, query, results, true)
}

/// Evaluates current query circumstances and potentially generates
/// prev page url if there is a potential for retrieving elements that precede
/// current resulting elements in query's sort.
fn get_prev_page(
    params: &QueryParams,
    query: &Query,
    results: &[JsonObject],
) -> Result<Option<String>> {
    get_page(params, query, results, false)
}

fn get_page(
    params: &QueryParams,
    query: &Query,
    results: &[JsonObject],
    forward: bool,
) -> Result<Option<String>> {
    if !results.is_empty() {
        let pivot = if forward {
            results.last().unwrap()
        } else {
            results.first().unwrap()
        };
        let cursor = cursor_from_pivot(query, pivot, forward)?;
        let rel_url = make_page_url(&params.url_path, &params.url_query, &cursor.to_string()?);
        return Ok(Some(rel_url));
    } else if let Some(cursor) = &query.cursor {
        if cursor.forward != forward {
            let cursor = cursor.reversed();
            let rel_url = make_page_url(&params.url_path, &params.url_query, &cursor.to_string()?);
            return Ok(Some(rel_url));
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
            .unwrap_or(serde_json::Value::Null);
        axes.push(CursorAxis { key, value });
    }
    let cursor = Cursor::new(axes, forward);
    Ok(cursor)
}

/// Generates relative URL that can be used to retrieve previous/next page.
/// It does this by using the path from the current `url` and setting the `cursor` as a query
/// parameter.
fn make_page_url(url_path: &str, url_query: &[(String, String)], cursor: &str) -> String {
    let mut page_query = form_urlencoded::Serializer::new(String::new());
    for (key, value) in url_query.iter() {
        if key == "cursor" || key == "sort" {
            continue;
        }
        page_query.append_pair(key, value);
    }
    page_query.append_pair("cursor", cursor);

    format!("{}?{}", url_path, page_query.finish())
}

/// Constructs Delete Mutation from CRUD url query.
pub fn delete_from_url_query(
    ctx: &DataContext,
    type_name: &str,
    url_query: &[(String, String)],
) -> Result<Mutation> {
    let base_entity = match ctx.type_system.lookup_type(type_name) {
        Ok(Type::Entity(ty)) => ty,
        Ok(ty) => anyhow::bail!("Cannot delete scalar type {type_name} ({})", ty.name()),
        Err(_) => anyhow::bail!("Cannot delete from type `{type_name}`, type not found"),
    };
    let filter_expr = url_query_to_filter(&base_entity, url_query, &ctx.type_system)
        .context("failed to convert crud URL to filter expression")?;
    if filter_expr.is_none() {
        let delete_all = url_query
            .iter()
            .any(|(key, value)| key == "all" && value == "true");
        if !delete_all {
            anyhow::bail!("crud delete requires a filter to be set or `all=true` parameter.")
        }
    }
    Mutation::delete_from_expr(ctx, type_name, &filter_expr)
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

    /// Parses provided `url_query` and builds a `Query` that can be used to build a `QueryPlan`.
    fn from_url_query(
        base_type: &Entity,
        url_query: &[(String, String)],
        ts: &TypeSystem,
    ) -> Result<Self> {
        let mut q = Query::new();
        for (param_key, value) in url_query.iter() {
            match param_key.as_str() {
                "sort" => q.sort = parse_sort(base_type, value)?,
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
                    let cursor = Cursor::from_string(value)?;
                    q.cursor = Some(cursor);
                }
                _ => {
                    if let Some(param_key) = param_key.strip_prefix('.') {
                        let expr = filter_from_param(base_type, param_key, value, ts)
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

    /// Makes query ops based on the CRUD parameters that were parsed by `from_url_query` method.
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
        Type::Entity(_) | Type::Array(_) => anyhow::bail!(
            "trying to filter by property of type '{}' which is not supported",
            field_type.name()
        ),
        Type::String => ExprValue::String(convert!(as_str, "string")),
        Type::Float | Type::JsDate => ExprValue::F64(convert!(as_f64, "float")),
        Type::Boolean => ExprValue::Bool(convert!(as_bool, "bool")),
        Type::EntityId { .. } => ExprValue::String(convert!(as_str, "string")),
    };
    Ok(expr_val.into())
}

/// Parses all CRUD query-string filters over `base_type` from provided `url_query`.
fn url_query_to_filter(
    base_type: &Entity,
    url_query: &[(String, String)],
    ts: &TypeSystem,
) -> Result<Option<Expr>> {
    let mut filter = None;
    for (param_key, value) in url_query.iter() {
        let param_key = param_key.to_string();
        if let Some(param_key) = param_key.strip_prefix('.') {
            let expression = filter_from_param(base_type, param_key, value, ts)
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
    let expr_value = match &field_type {
        Type::String => ExprValue::String(value.to_owned()),
        Type::Float | Type::JsDate => {
            ExprValue::F64(value.parse::<f64>().with_context(|| err_msg("f64"))?)
        }
        Type::Boolean => ExprValue::Bool(value.parse::<bool>().with_context(|| err_msg("bool"))?),
        Type::EntityId { .. } => ExprValue::String(value.to_owned()),
        Type::Entity(_) | Type::Array(_) => anyhow::bail!(
            "trying to filter by property '{}' of type '{}' which is not supported",
            fields.last().unwrap(),
            field_type.name()
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

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use serde_json::json;
    use url::Url;

    use super::*;
    use crate::datastore::engine::QueryEngine;
    use crate::datastore::query::tests::{add_row, binary, fetch_rows, setup_clear_db};
    use crate::datastore::test::{
        collect_names, make_entity, make_field, COMPANY_TY, ENTITIES, PERSON_TY, TYPE_SYSTEM,
    };
    use crate::types::{FieldDescriptor, ObjectDescriptor};

    pub struct FakeField {
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
        fn version_id(&self) -> String {
            "whatever".to_string()
        }
    }

    pub struct FakeObject {
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
        fn version_id(&self) -> String {
            "whatever".to_string()
        }
    }

    fn url(query_string: &str) -> Url {
        Url::parse(&format!("http://xxx?{}", query_string)).unwrap()
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
        let filter_expr = |key: &str, value: &str| -> Expr {
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

    #[tokio::test]
    async fn test_run_query() {
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        let john = json!({"name": "John", "age": json!(20f32)});
        let steve = json!({"name": "Steve", "age": json!(29f32)});
        let (query_engine, _db_file) = setup_clear_db(&*ENTITIES).await;
        let qe = &query_engine;
        qe.with_dummy_ctx(Default::default(), |ctx| async {
            add_row(qe, &PERSON_TY, &alan, &ctx).await;
            add_row(qe, &PERSON_TY, &john, &ctx).await;
            add_row(qe, &PERSON_TY, &steve, &ctx).await;
            ctx
        })
        .await;

        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let mut r = qe.run_query_vec(&ctx, "Person", url("")).await;
            r.sort();
            assert_eq!(r, vec!["Alan", "John", "Steve"]);

            let r = qe.run_query_vec(&ctx, "Person", url("limit=2")).await;
            assert_eq!(r.len(), 2);

            let r = qe.run_query_vec(&ctx, "Person", url("page_size=2")).await;
            assert_eq!(r.len(), 2);

            // Test Sorting
            let r = qe.run_query_vec(&ctx, "Person", url("sort=age")).await;
            assert_eq!(r, vec!["John", "Steve", "Alan"]);

            let r = qe.run_query_vec(&ctx, "Person", url("sort=%2Bage")).await;
            assert_eq!(r, vec!["John", "Steve", "Alan"]);

            let r = qe.run_query_vec(&ctx, "Person", url("sort=-age")).await;
            assert_eq!(r, vec!["Alan", "Steve", "John"]);

            // Test Offset
            let r = qe
                .run_query_vec(&ctx, "Person", url("sort=name&offset=1"))
                .await;
            assert_eq!(r, vec!["John", "Steve"]);

            // Test filtering
            let r = qe.run_query_vec(&ctx, "Person", url(".age=10")).await;
            assert_eq!(r, Vec::<String>::new());

            let r = qe.run_query_vec(&ctx, "Person", url(".age=29")).await;
            assert_eq!(r, vec!["Steve"]);

            let r = qe
                .run_query_vec(&ctx, "Person", url(".age~lt=29&.name~like=%25n"))
                .await;
            assert_eq!(r, vec!["John"]);

            let mut r = qe
                .run_query_vec(&ctx, "Person", url(".name~unlike=Al%25"))
                .await;
            r.sort();
            assert_eq!(r, vec!["John", "Steve"]);

            ctx
        })
        .await;

        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let alex = json!({"name": "Alex", "age": json!(40f32)});
            add_row(qe, &PERSON_TY, &alex, &ctx).await;
            ctx
        })
        .await;

        // Test permutations of parameters
        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let raw_ops = vec!["page_size=2", "offset=1", "sort=age"];
            for perm in raw_ops.iter().permutations(raw_ops.len()) {
                let query_string = perm.iter().join("&");
                let r = qe.run_query_vec(&ctx, "Person", url(&query_string)).await;
                assert_eq!(
                    r,
                    vec!["Steve", "Alan"],
                    "unexpected result for query string '{query_string}'",
                );
            }

            let raw_ops = vec!["page_size=2", "offset=1", "sort=name", ".age~gt=20"];
            for perm in raw_ops.iter().permutations(raw_ops.len()) {
                let query_string = perm.iter().join("&");
                let r = qe.run_query_vec(&ctx, "Person", url(&query_string)).await;
                assert_eq!(
                    r,
                    vec!["Alex", "Steve"],
                    "unexpected result for query string '{query_string}'",
                );
            }
            ctx
        })
        .await;

        // Test multiple levels of indirection
        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let chiselstrike = json!({"name": "ChiselStrike", "ceo": john});
            let thunderstrike = json!({"name": "ThunderStrike", "ceo": alan});
            add_row(qe, &COMPANY_TY, &chiselstrike, &ctx).await;
            add_row(qe, &COMPANY_TY, &thunderstrike, &ctx).await;

            let r = qe
                .run_query_vec(&ctx, "Company", url(".ceo.age~lte=20"))
                .await;
            assert_eq!(r, vec!["ChiselStrike"]);
            ctx
        })
        .await;
    }

    #[tokio::test]
    async fn test_paging() {
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        let alex = json!({"name": "Alex", "age": json!(40f32)});
        let john = json!({"name": "John", "age": json!(20f32)});
        let steve = json!({"name": "Steve", "age": json!(29f32)});

        let (query_engine, _db_file) = setup_clear_db(&*ENTITIES).await;
        let qe = &query_engine;
        qe.with_dummy_ctx(Default::default(), |ctx| async {
            add_row(qe, &PERSON_TY, &alan, &ctx).await;
            add_row(qe, &PERSON_TY, &john, &ctx).await;
            add_row(qe, &PERSON_TY, &steve, &ctx).await;
            add_row(qe, &PERSON_TY, &alex, &ctx).await;
            ctx
        })
        .await;

        fn get_url(raw: &serde_json::Value) -> Url {
            Url::parse("http://localhost")
                .unwrap()
                .join(raw.as_str().unwrap())
                .unwrap()
        }

        // Simple forward step.
        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let r = qe
                .run_test_query(&ctx, "Person", url("sort=name&page_size=2"))
                .await
                .unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan", "Alex"]);

            assert!(r.contains_key("next_page"));
            let next_page = get_url(&r["next_page"]);
            let r = qe.run_test_query(&ctx, "Person", next_page).await.unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["John", "Steve"]);
            ctx
        })
        .await;

        // Empty pre-first page loop
        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let r = qe
                .run_test_query(&ctx, "Person", url("sort=name&page_size=1"))
                .await
                .unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan"]);

            assert!(r.contains_key("prev_page"));
            let prev_page = get_url(&r["prev_page"]);

            let r = qe
                .run_test_query(&ctx, "Person", prev_page.clone())
                .await
                .unwrap();
            let names = collect_names(&r);
            assert!(names.is_empty());

            assert!(r.contains_key("next_page"));
            let first_page = get_url(&r["next_page"]);
            let r = qe
                .run_test_query(&ctx, "Person", first_page.clone())
                .await
                .unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan"]);
            ctx
        })
        .await;

        // Empty last page loop
        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let r = qe
                .run_test_query(&ctx, "Person", url("sort=name&page_size=4"))
                .await
                .unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan", "Alex", "John", "Steve"]);

            assert!(r.contains_key("next_page"));
            let next_page = get_url(&r["next_page"]);

            let r = qe
                .run_test_query(&ctx, "Person", next_page.clone())
                .await
                .unwrap();
            let names = collect_names(&r);
            assert!(names.is_empty());

            assert!(r.contains_key("prev_page"));
            let last_page = get_url(&r["prev_page"]);
            let r = qe
                .run_test_query(&ctx, "Person", last_page.clone())
                .await
                .unwrap();
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan", "Alex", "John", "Steve"]);
            ctx
        })
        .await;

        async fn run_cursor_test(
            qe: &QueryEngine,
            ctx: &DataContext,
            mut page_url: Url,
            n_steps: usize,
            page_size: usize,
        ) -> Vec<String> {
            let mut all_names = vec![];
            for i in 0..n_steps {
                let r = qe
                    .run_test_query(ctx, "Person", page_url.clone())
                    .await
                    .unwrap();

                // Check backward cursors
                if i == 0 {
                    assert!(r.contains_key("prev_page"));
                } else if i != 0 {
                    // Check that previous page returns the same results as
                    // before using next_page.
                    let prev_page = get_url(&r["prev_page"]);
                    let r = qe
                        .run_test_query(ctx, "Person", prev_page.clone())
                        .await
                        .unwrap();
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
                        let r = qe
                            .run_test_query(ctx, "Person", prev_page.clone())
                            .await
                            .unwrap();
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

        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let mut all_names = run_cursor_test(qe, &ctx, url("page_size=1"), 5, 1).await;
            all_names.sort();
            assert_eq!(all_names, vec!["Alan", "Alex", "John", "Steve"]);
            ctx
        })
        .await;

        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let all_names = run_cursor_test(qe, &ctx, url("sort=name&page_size=2"), 3, 2).await;
            assert_eq!(all_names, vec!["Alan", "Alex", "John", "Steve"]);
            ctx
        })
        .await;

        qe.with_dummy_ctx(Default::default(), |ctx| async {
            let r = qe
                .run_test_query(&ctx, "Person", url("sort=name&page_size=5"))
                .await
                .unwrap();

            assert!(r.contains_key("next_page"));
            assert!(r.contains_key("prev_page"));
            let names = collect_names(&r);
            assert_eq!(names, vec!["Alan", "Alex", "John", "Steve"]);
            ctx
        })
        .await;
    }

    #[tokio::test]
    async fn test_query_str_to_ops_errors() {
        let (query_engine, _db_file) = setup_clear_db(&*ENTITIES).await;
        let qe = &query_engine;
        qe.with_dummy_ctx(Default::default(), |ctx| async {
            assert!(qe
                .run_test_query(&ctx, "Person", url("limit=two"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url("limit=true"))
                .await
                .is_err());

            assert!(qe
                .run_test_query(&ctx, "Person", url("offset=two"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url("offset=true"))
                .await
                .is_err());

            assert!(qe
                .run_test_query(&ctx, "Person", url("sort=age1"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url("sort=%2Bnotname"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url("sort=-notname"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url("sort=--age"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url("sort=age aa"))
                .await
                .is_err());

            assert!(qe
                .run_test_query(&ctx, "Person", url(".agex=4"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url("..age=4"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url(".age=four"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url(".age=true"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url(".age~ltt=4"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url(".age~neq~lt=4"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url(".age.nothing=4"))
                .await
                .is_err());
            assert!(qe
                .run_test_query(&ctx, "Person", url(".=123"))
                .await
                .is_err());
            ctx
        })
        .await
    }

    #[tokio::test]
    async fn test_delete_from_crud_url() {
        let delete_from_url_query = |ctx: &DataContext, entity_name: &str, query: &str| {
            let url_query: Vec<_> = form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect();
            delete_from_url_query(ctx, entity_name, &url_query).unwrap()
        };

        let john = json!({"name": "John", "age": json!(20f32)});
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            qe.with_dummy_ctx(Default::default(), |ctx| async {
                add_row(&qe, &PERSON_TY, &john, &ctx).await;
                ctx
            })
            .await;

            qe.with_dummy_ctx(Default::default(), |ctx| async {
                let mutation = delete_from_url_query(&ctx, "Person", ".name=John");
                {
                    let mut txn = ctx.txn.lock().await;
                    qe.mutate_with_transaction(mutation, &mut txn)
                        .await
                        .unwrap();
                }

                assert_eq!(fetch_rows(&qe, ctx.txn.clone(), &PERSON_TY).await.len(), 0);
                ctx
            })
            .await;
        }

        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            qe.with_dummy_ctx(Default::default(), |ctx| async {
                add_row(&qe, &PERSON_TY, &john, &ctx).await;
                add_row(&qe, &PERSON_TY, &alan, &ctx).await;
                ctx
            })
            .await;

            qe.with_dummy_ctx(Default::default(), |ctx| async {
                let mutation = delete_from_url_query(&ctx, "Person", ".age=30");
                {
                    let mut txn = ctx.txn.lock().await;
                    qe.mutate_with_transaction(mutation, &mut txn)
                        .await
                        .unwrap();
                }

                let rows = fetch_rows(&qe, ctx.txn.clone(), &PERSON_TY).await;
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0]["name"], "John");
                ctx
            })
            .await;
        }

        let chiselstrike = json!({"name": "ChiselStrike", "ceo": john});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            qe.with_dummy_ctx(Default::default(), |ctx| async {
                add_row(&qe, &COMPANY_TY, &chiselstrike, &ctx).await;
                ctx
            })
            .await;

            qe.with_dummy_ctx(Default::default(), |ctx| async {
                let mutation = delete_from_url_query(&ctx, "Company", ".ceo.name=John");
                {
                    let mut txn = ctx.txn.lock().await;
                    qe.mutate_with_transaction(mutation, &mut txn)
                        .await
                        .unwrap();
                }

                assert_eq!(fetch_rows(&qe, ctx.txn.clone(), &COMPANY_TY).await.len(), 0);
                ctx
            })
            .await;
        }
    }

    #[test]
    fn test_make_page_url() {
        fn check(url_path: &str, url_query: &str, cursor: &str, expected: &str) {
            let url_query: Vec<_> = form_urlencoded::parse(url_query.as_bytes())
                .into_owned()
                .collect();
            let actual = make_page_url(url_path, &url_query, cursor);
            assert_eq!(actual.as_str(), expected);
        }

        check("/path", "", "abcd", "/path?cursor=abcd");
        check("/path", "really=no", "xyzw", "/path?really=no&cursor=xyzw");
        check(
            "/path",
            "really=no&foo=bar",
            "xyzw",
            "/path?really=no&foo=bar&cursor=xyzw",
        );
        check(
            "/longer/url/path",
            "",
            "abcd",
            "/longer/url/path?cursor=abcd",
        );
    }
}
