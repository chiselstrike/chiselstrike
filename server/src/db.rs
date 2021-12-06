// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::types::Type;
use anyhow::{anyhow, Result};
use itertools::Itertools;

#[derive(Debug)]
enum Inner {
    BackingStore(String),
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

fn convert_backing_store(val: &serde_json::Value) -> Result<Relation> {
    let name = val["name"].as_str().ok_or_else(|| anyhow!("foo"))?;
    let columns = get_columns(val)?;
    Ok(Relation {
        columns,
        inner: Inner::BackingStore(name.to_string()),
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

fn convert_filter(val: &serde_json::Value) -> Result<Relation> {
    let columns = get_columns(val)?;
    let inner = Box::new(convert(&val["inner"])?);
    let restrictions = val["restrictions"]
        .as_object()
        .ok_or_else(|| anyhow!("Missing restrictions in filter"))?;
    let mut restriction_strs = vec![];
    for (k, v) in restrictions.iter() {
        // FIXME: Support non-strings
        let v = v
            .as_str()
            .ok_or_else(|| anyhow!("Restriction is not a string"))?;
        restriction_strs.push(format!("{}='{}'", k, v));
    }
    Ok(Relation {
        columns,
        inner: Inner::Filter(inner, restriction_strs),
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

fn sql_impl(rel: &Relation, alias_count: &mut u32) -> String {
    let mut names = rel.columns.iter().map(|c| &c.0);
    let col_str = names.join(",");
    match &rel.inner {
        Inner::BackingStore(name) => {
            format!("SELECT {} FROM {}", col_str, name)
        }
        Inner::Filter(inner, restrictions) => {
            let inner_sql = sql_impl(inner, alias_count);
            let inner_alias = format!("A{}", *alias_count);
            *alias_count += 1;
            let restrictions = restrictions.join(",");
            format!(
                "SELECT {} FROM ({}) AS {} WHERE {}",
                col_str, inner_sql, inner_alias, restrictions
            )
        }
        Inner::Join(left, right) => {
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
    }
}

pub(crate) fn sql(rel: &Relation) -> String {
    let mut v = 0;
    sql_impl(rel, &mut v)
}
