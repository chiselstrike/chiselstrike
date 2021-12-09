// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::deno::get_policies;
use crate::policies::FieldPolicies;
use crate::query::engine;
use crate::query::engine::QueryResults;
use crate::query::engine::RawSqlStream;
use crate::query::engine::SqlStream;
use crate::runtime;
use crate::types::Type;
use anyhow::{anyhow, Result};
use async_recursion::async_recursion;
use futures::Stream;
use itertools::Itertools;
use serde_json::value::Value;
use sqlx::AnyPool;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

#[derive(Debug)]
enum Inner {
    BackingStore(String, FieldPolicies),
    Join(Box<Relation>, Box<Relation>),
    Filter(Box<Relation>, Vec<String>),
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

async fn convert_backing_store(val: &serde_json::Value) -> Result<Relation> {
    let name = val["name"].as_str().ok_or_else(|| anyhow!("foo"))?;
    let columns = get_columns(val)?;
    let runtime = &mut runtime::get().await;
    let ts = &runtime.type_system;
    let ty = ts.lookup_object_type(name)?;
    let policies = get_policies(runtime, &ty).await?;

    Ok(Relation {
        columns,
        inner: Inner::BackingStore(ty.backing_table().to_string(), policies),
    })
}

#[async_recursion(?Send)]
async fn convert_join(val: &serde_json::Value) -> Result<Relation> {
    let columns = get_columns(val)?;
    let left = Box::new(convert(&val["left"]).await?);
    let right = Box::new(convert(&val["right"]).await?);
    Ok(Relation {
        columns,
        inner: Inner::Join(left, right),
    })
}

// FIXME: We should use prepared statements instead
fn escape_string(s: &str) -> String {
    format!("{}", format_sql_query::QuotedData(s))
}

#[async_recursion(?Send)]
async fn convert_filter(val: &serde_json::Value) -> Result<Relation> {
    let columns = get_columns(val)?;
    let inner = Box::new(convert(&val["inner"]).await?);
    let restrictions = val["restrictions"]
        .as_object()
        .ok_or_else(|| anyhow!("Missing restrictions in filter"))?;
    let mut restriction_strs = vec![];
    for (k, v) in restrictions.iter() {
        let v = match v {
            Value::Null => anyhow::bail!("Null restriction"),
            Value::Bool(v) => format!("{}", v),
            Value::Number(v) => format!("{}", v),
            Value::String(v) => escape_string(v),
            Value::Array(v) => anyhow::bail!("Array restriction {:?}", v),
            Value::Object(v) => anyhow::bail!("Object restriction {:?}", v),
        };
        restriction_strs.push(format!("{}={}", k, v));
    }
    Ok(Relation {
        columns,
        inner: Inner::Filter(inner, restriction_strs),
    })
}

pub(crate) async fn convert(val: &serde_json::Value) -> Result<Relation> {
    let kind = val["kind"].as_str().ok_or_else(|| anyhow!("foo"))?;
    match kind {
        "BackingStore" => convert_backing_store(val).await,
        "Join" => convert_join(val).await,
        "Filter" => convert_filter(val).await,
        _ => Err(anyhow!("Unexpected relation kind")),
    }
}

fn column_list(rel: &Relation) -> String {
    let mut names = rel.columns.iter().map(|c| &c.0);
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
    type Item = anyhow::Result<serde_json::Value>;
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
    rel: &Relation,
    name: &str,
    policies: &FieldPolicies,
) -> Query {
    let query = format!("SELECT {} FROM {}", column_list(rel), name);
    if policies.is_empty() {
        return Query::Sql(query);
    }
    let stream = QueryResults::new(query, pool);
    let pstream = Box::pin(PolicyApplyingStream {
        inner: stream,
        policies: policies.clone(),
        columns: rel.columns.clone(),
    });
    Query::Stream(pstream)
}

fn sql_filter(
    pool: &AnyPool,
    rel: &Relation,
    alias_count: &mut u32,
    inner: &Relation,
    restrictions: &[String],
) -> Query {
    let inner_sql = sql_impl(pool, inner, alias_count);
    let inner_sql = match inner_sql {
        Query::Sql(s) => s,
        Query::Stream(_) => unimplemented!(),
    };
    let inner_alias = format!("A{}", *alias_count);
    *alias_count += 1;
    let restrictions = restrictions.join(" AND ");
    Query::Sql(format!(
        "SELECT {} FROM ({}) AS {} WHERE {}",
        column_list(rel),
        inner_sql,
        inner_alias,
        restrictions
    ))
}

fn sql_join(
    pool: &AnyPool,
    rel: &Relation,
    alias_count: &mut u32,
    left: &Relation,
    right: &Relation,
) -> Query {
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

    let left_alias = format!("A{}", *alias_count);
    let right_alias = format!("A{}", *alias_count + 1);
    *alias_count += 2;

    let mut join_columns = vec![];
    let mut on_columns = vec![];
    for c in &rel.columns {
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

fn sql_impl(pool: &AnyPool, rel: &Relation, alias_count: &mut u32) -> Query {
    match &rel.inner {
        Inner::BackingStore(name, policies) => sql_backing_store(pool, rel, name, policies),
        Inner::Filter(inner, restrictions) => {
            sql_filter(pool, rel, alias_count, inner, restrictions)
        }
        Inner::Join(left, right) => sql_join(pool, rel, alias_count, left, right),
    }
}

pub(crate) fn sql(pool: &AnyPool, rel: &Relation) -> SqlStream {
    let mut v = 0;
    match sql_impl(pool, rel, &mut v) {
        Query::Sql(s) => {
            let inner = QueryResults::new(s, pool);
            Box::pin(PolicyApplyingStream {
                inner,
                policies: FieldPolicies::new(),
                columns: rel.columns.clone(),
            })
        }
        Query::Stream(s) => s,
    }
}
