// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Result};

#[derive(Debug)]
enum Inner {
    BackingStore(String),
    Join(Box<Relation>, Box<Relation>),
}

#[derive(Debug)]
pub(crate) struct Relation {
    columns: Vec<String>,
    inner: Inner,
}

fn get_columns(val: &serde_json::Value) -> Result<Vec<String>> {
    let columns = val["columns"].as_array().ok_or_else(|| anyhow!("foo"))?;
    let mut ret = vec![];
    for c in columns {
        let c = c.as_str().ok_or_else(|| anyhow!("foo"))?;
        ret.push(c.to_string());
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

pub(crate) fn convert(val: &serde_json::Value) -> Result<Relation> {
    let kind = val["kind"].as_str().ok_or_else(|| anyhow!("foo"))?;
    match kind {
        "BackingStore" => convert_backing_store(val),
        "Join" => convert_join(val),
        _ => Err(anyhow!("bar")),
    }
}

fn sql_impl(rel: &Relation, alias_count: &mut u32) -> String {
    match &rel.inner {
        Inner::BackingStore(name) => {
            let col_str = rel.columns.join(",");
            format!("SELECT {} FROM {}", col_str, name)
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
                    join_columns.push(format!("${}.${}", left_alias, c));
                    on_columns.push(format!("{}.${} = ${}.${}", left_alias, c, right_alias, c));
                } else {
                    join_columns.push(c.clone());
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
