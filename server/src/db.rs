// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::deno::get_policies;
use crate::policies::FieldPolicies;
use crate::runtime;
use crate::types::Type;
use anyhow::{anyhow, Result};
use async_recursion::async_recursion;
use itertools::Itertools;
use serde_json::value::Value;

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
            Value::String(v) => v.clone(),
            Value::Array(v) => anyhow::bail!("Array restriction {:?}", v),
            Value::Object(v) => anyhow::bail!("Object restriction {:?}", v),
        };
        restriction_strs.push(format!("{}='{}'", k, v));
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

fn sql_backing_store(rel: &Relation, name: &str, _policies: &FieldPolicies) -> String {
    format!("SELECT {} FROM {}", column_list(rel), name)
}

fn sql_filter(
    rel: &Relation,
    alias_count: &mut u32,
    inner: &Relation,
    restrictions: &[String],
) -> String {
    let inner_sql = sql_impl(inner, alias_count);
    let inner_alias = format!("A{}", *alias_count);
    *alias_count += 1;
    let restrictions = restrictions.join(" AND ");
    format!(
        "SELECT {} FROM ({}) AS {} WHERE {}",
        column_list(rel),
        inner_sql,
        inner_alias,
        restrictions
    )
}

fn sql_join(rel: &Relation, alias_count: &mut u32, left: &Relation, right: &Relation) -> String {
    // FIXME: Optimize the case of table.left or table.right being just
    // a BackingStore with all fields. The database probably doesn't
    // care, but will make the logs cleaner.
    let lsql = sql_impl(left, alias_count);
    let rsql = sql_impl(right, alias_count);

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
    return format!("SELECT {} FROM {} ON {}", join_columns_str, join, on);
}

fn sql_impl(rel: &Relation, alias_count: &mut u32) -> String {
    match &rel.inner {
        Inner::BackingStore(name, policies) => sql_backing_store(rel, name, policies),
        Inner::Filter(inner, restrictions) => sql_filter(rel, alias_count, inner, restrictions),
        Inner::Join(left, right) => sql_join(rel, alias_count, left, right),
    }
}

pub(crate) fn sql(rel: &Relation) -> String {
    let mut v = 0;
    sql_impl(rel, &mut v)
}
