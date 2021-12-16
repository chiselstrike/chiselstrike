// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::deno::current_api_version;
use crate::deno::get_policies;
use crate::policies::FieldPolicies;
use crate::query::engine;
use crate::query::engine::new_query_results;
use crate::query::engine::JsonObject;
use crate::query::engine::RawSqlStream;
use crate::query::engine::SqlStream;
use crate::runtime;
use crate::types::Type;
use anyhow::{anyhow, Result};
use futures::future;
use futures::Stream;
use futures::StreamExt;
use itertools::Itertools;
use serde_json::value::Value;
use sqlx::AnyPool;
use std::collections::HashMap;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

#[derive(Debug)]
enum SqlValue {
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    String(String),
}

#[derive(Debug)]
struct Restriction {
    k: String,
    v: SqlValue,
}

#[derive(Debug)]
enum Inner {
    BackingStore(String, FieldPolicies),
    Join(Box<Relation>, Box<Relation>),
    Filter(Box<Relation>, Vec<Restriction>),
}

#[derive(Debug)]
pub(crate) struct Relation {
    // FIXME: This can't be a Type::Object, we should probably split the enum
    pub(crate) columns: Vec<(String, Type)>,
    inner: Inner,
}

fn get_columns(val: &serde_json::Value) -> Result<Vec<(String, Type)>> {
    let columns = val["columns"].as_array().ok_or_else(|| anyhow!("foo"))?;
    let mut ret = vec![];
    for c in columns {
        let c = c
            .as_array()
            .ok_or_else(|| anyhow!("colums should be arrays"))?;
        anyhow::ensure!(c.len() == 2, "colums should have a name and a type");
        let name = c[0]
            .as_str()
            .ok_or_else(|| anyhow!("name should be a string"))?;
        let type_ = c[1]
            .as_str()
            .ok_or_else(|| anyhow!("type should be a string"))?;
        let type_ = match type_ {
            "number" => Type::Float,
            "bigint" => Type::Int,
            "string" => Type::String,
            "boolean" => Type::Boolean,
            v => anyhow::bail!("Invalid type {}", v),
        };
        ret.push((name.to_string(), type_));
    }
    Ok(ret)
}

fn convert_backing_store(val: &serde_json::Value) -> Result<Relation> {
    let name = val["name"].as_str().ok_or_else(|| anyhow!("foo"))?;
    let columns = get_columns(val)?;
    let runtime = runtime::get();
    let ts = &runtime.type_system;
    let api_version = current_api_version();
    let ty = ts.lookup_object_type(name, &api_version)?;
    let policies = get_policies(&runtime, &ty)?;

    Ok(Relation {
        columns,
        inner: Inner::BackingStore(ty.backing_table().to_string(), policies),
    })
}

fn convert_join(val: &serde_json::Value) -> Result<Relation> {
    let columns = get_columns(val)?;
    let left = Box::new(convert(&val["left"])?);
    let right = Box::new(convert(&val["right"])?);
    Ok(Relation {
        columns,
        inner: Inner::Join(left, right),
    })
}

// FIXME: We should use prepared statements instead
fn escape_string(s: &str) -> String {
    format!("{}", format_sql_query::QuotedData(s))
}

fn convert_filter(val: &serde_json::Value) -> Result<Relation> {
    let columns = get_columns(val)?;
    let inner = Box::new(convert(&val["inner"])?);
    let restrictions = val["restrictions"]
        .as_object()
        .ok_or_else(|| anyhow!("Missing restrictions in filter"))?;
    let mut sql_restrictions = vec![];
    for (k, v) in restrictions.iter() {
        let v = match v {
            Value::Null => anyhow::bail!("Null restriction"),
            Value::Bool(v) => SqlValue::Bool(*v),
            Value::Number(v) => {
                if let Some(v) = v.as_u64() {
                    SqlValue::U64(v)
                } else if let Some(v) = v.as_i64() {
                    SqlValue::I64(v)
                } else {
                    SqlValue::F64(v.as_f64().unwrap())
                }
            }
            Value::String(v) => SqlValue::String(v.clone()),
            Value::Array(v) => anyhow::bail!("Array restriction {:?}", v),
            Value::Object(v) => anyhow::bail!("Object restriction {:?}", v),
        };
        sql_restrictions.push(Restriction { k: k.clone(), v });
    }
    Ok(Relation {
        columns,
        inner: Inner::Filter(inner, sql_restrictions),
    })
}

pub(crate) fn convert(val: &serde_json::Value) -> Result<Relation> {
    let kind = val["kind"].as_str().ok_or_else(|| anyhow!("foo"))?;
    match kind {
        "BackingStore" => convert_backing_store(val),
        "Join" => convert_join(val),
        "Filter" => convert_filter(val),
        _ => Err(anyhow!("Unexpected relation kind")),
    }
}

fn column_list(columns: &[(String, Type)]) -> String {
    let mut names = columns.iter().map(|c| &c.0);
    names.join(",")
}

enum Query {
    Sql(String),
    Stream(SqlStream),
}

struct PolicyApplyingStream {
    inner: RawSqlStream,
    policies: FieldPolicies,
    columns: Vec<(String, Type)>,
}

impl Stream for PolicyApplyingStream {
    type Item = anyhow::Result<JsonObject>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let columns = self.columns.clone();
        let policies = self.policies.clone();
        // Structural Pinning, it is OK because inner is pinned when we are.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        match futures::ready!(inner.poll_next(cx)) {
            None => Poll::Ready(None),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            Some(Ok(item)) => {
                let mut v = engine::relational_row_to_json(&columns, &item)?;
                for (field, xform) in &policies {
                    v[field] = xform(v[field].take());
                }
                Poll::Ready(Some(Ok(v)))
            }
        }
    }
}

fn sql_backing_store(
    pool: &AnyPool,
    columns: Vec<(String, Type)>,
    name: String,
    policies: FieldPolicies,
) -> Query {
    let query = format!("SELECT {} FROM {}", column_list(&columns), name);
    if policies.is_empty() {
        return Query::Sql(query);
    }
    let stream = new_query_results(query, pool);
    let pstream = Box::pin(PolicyApplyingStream {
        inner: stream,
        policies,
        columns,
    });
    Query::Stream(pstream)
}

fn filter_stream_item(
    o: &anyhow::Result<JsonObject>,
    restrictions: &HashMap<String, SqlValue>,
) -> bool {
    let o = match o {
        Ok(o) => o,
        Err(_) => return true,
    };
    for (k, v) in o.iter() {
        if let Some(v2) = restrictions.get(k) {
            let eq = match v2 {
                SqlValue::Bool(v2) => v == v2,
                SqlValue::U64(v2) => v == v2,
                SqlValue::I64(v2) => v == v2,
                SqlValue::F64(v2) => v == v2,
                SqlValue::String(v2) => v == v2,
            };
            if !eq {
                return false;
            }
        }
    }
    true
}

fn filter_stream(stream: SqlStream, restrictions: Vec<Restriction>) -> Query {
    let restrictions: HashMap<String, SqlValue> =
        restrictions.into_iter().map(|r| (r.k, r.v)).collect();
    let stream = stream.filter(move |o| future::ready(filter_stream_item(o, &restrictions)));
    Query::Stream(Box::pin(stream))
}

fn sql_filter(
    pool: &AnyPool,
    columns: &[(String, Type)],
    alias_count: &mut u32,
    inner: Relation,
    restrictions: Vec<Restriction>,
) -> Query {
    let inner_sql = sql_impl(pool, inner, alias_count);
    let inner_sql = match inner_sql {
        Query::Sql(s) => s,
        Query::Stream(s) => return filter_stream(s, restrictions),
    };
    let inner_alias = format!("A{}", *alias_count);
    *alias_count += 1;

    let restrictions = restrictions
        .iter()
        .map(|rest| {
            let str_v = match &rest.v {
                SqlValue::Bool(v) => format!("{}", v),
                SqlValue::U64(v) => format!("{}", v),
                SqlValue::I64(v) => format!("{}", v),
                SqlValue::F64(v) => format!("{}", v),
                SqlValue::String(v) => escape_string(v),
            };
            format!("{}={}", rest.k, str_v)
        })
        .join(" AND ");
    Query::Sql(format!(
        "SELECT {} FROM ({}) AS {} WHERE {}",
        column_list(columns),
        inner_sql,
        inner_alias,
        restrictions
    ))
}

fn sql_join(
    pool: &AnyPool,
    columns: &[(String, Type)],
    alias_count: &mut u32,
    left: Relation,
    right: Relation,
) -> Query {
    let left_alias = format!("A{}", *alias_count);
    let right_alias = format!("A{}", *alias_count + 1);
    *alias_count += 2;

    let mut join_columns = vec![];
    let mut on_columns = vec![];
    for c in columns {
        if left.columns.contains(c) && right.columns.contains(c) {
            join_columns.push(format!("${}.${}", left_alias, c.0));
            on_columns.push(format!(
                "{}.${} = ${}.${}",
                left_alias, c.0, right_alias, c.0
            ));
        } else {
            join_columns.push(c.0.clone());
        }
    }

    // FIXME: Optimize the case of table.left or table.right being just
    // a BackingStore with all fields. The database probably doesn't
    // care, but will make the logs cleaner.
    let lsql = sql_impl(pool, left, alias_count);
    let lsql = match lsql {
        Query::Sql(s) => s,
        Query::Stream(_) => unimplemented!(),
    };
    let rsql = sql_impl(pool, right, alias_count);
    let rsql = match rsql {
        Query::Sql(s) => s,
        Query::Stream(_) => unimplemented!(),
    };

    let on = if on_columns.is_empty() {
        "TRUE".to_string()
    } else {
        on_columns.join(" AND ")
    };
    // Funny way to write it, but works on PostgreSQL and sqlite.
    let join = format!(
        "({}) AS {} JOIN ({}) AS {}",
        lsql, left_alias, rsql, right_alias
    );
    let join_columns_str = join_columns.join(",");
    Query::Sql(format!(
        "SELECT {} FROM {} ON {}",
        join_columns_str, join, on
    ))
}

fn sql_impl(pool: &AnyPool, rel: Relation, alias_count: &mut u32) -> Query {
    match rel.inner {
        Inner::BackingStore(name, policies) => sql_backing_store(pool, rel.columns, name, policies),
        Inner::Filter(inner, restrictions) => {
            sql_filter(pool, &rel.columns, alias_count, *inner, restrictions)
        }
        Inner::Join(left, right) => sql_join(pool, &rel.columns, alias_count, *left, *right),
    }
}

pub(crate) fn sql(pool: &AnyPool, rel: Relation) -> SqlStream {
    let mut v = 0;
    let columns = rel.columns.clone();
    match sql_impl(pool, rel, &mut v) {
        Query::Sql(s) => {
            let inner = new_query_results(s, pool);
            Box::pin(PolicyApplyingStream {
                inner,
                policies: FieldPolicies::new(),
                columns,
            })
        }
        Query::Stream(s) => s,
    }
}
