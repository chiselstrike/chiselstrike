// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::db::{sql, Relation};
use crate::query::{DbConnection, Kind, QueryError};
use crate::types::{ObjectType, Type};
use anyhow::Result;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use itertools::zip;
use itertools::Itertools;
use sea_query::{Alias, ColumnDef, Expr, PostgresQueryBuilder, Query, SqliteQueryBuilder, Table};
use serde_json::json;
use sqlx::any::{Any, AnyPool, AnyRow};
use sqlx::error::Error;
use sqlx::Column;
use sqlx::{Executor, Row};
use std::cell::RefCell;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) struct QueryResults {
    raw_query: String,
    // The streams we use in here only depend on the lifetime of raw_query.
    stream: RefCell<Option<BoxStream<'static, Result<AnyRow, Error>>>>,
    _marker: PhantomPinned, // QueryStream cannot be moved
}

impl QueryResults {
    pub(crate) fn new(raw_query: String, pool: &AnyPool) -> Pin<Box<Self>> {
        let ret = Box::pin(QueryResults {
            raw_query,
            stream: RefCell::new(None),
            _marker: PhantomPinned,
        });
        let ptr: *const String = &ret.raw_query;
        let query = sqlx::query::<Any>(unsafe { &*ptr });
        let stream = query.fetch(pool);
        ret.stream.replace(Some(stream));
        ret
    }
}

impl Stream for QueryResults {
    type Item = Result<AnyRow, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut borrow = self.stream.borrow_mut();
        borrow.as_mut().unwrap().as_mut().poll_next(cx)
    }
}

/// Query engine.
///
/// The query engine provides a way to persist objects and retrieve them from
/// a backing store for ChiselStrike endpoints.
#[derive(Clone)]
pub(crate) struct QueryEngine {
    kind: Kind,
    pool: AnyPool,
}

impl QueryEngine {
    fn new(kind: Kind, pool: AnyPool) -> Self {
        Self { kind, pool }
    }

    pub(crate) async fn local_connection(conn: &DbConnection) -> anyhow::Result<Self> {
        let local = conn.local_connection().await?;
        Ok(Self::new(local.kind, local.pool))
    }

    pub(crate) async fn drop_table(&self, ty: &ObjectType) -> anyhow::Result<()> {
        let drop_table = Table::drop()
            .table(Alias::new(&ty.backing_table))
            .to_owned();
        let drop_table = drop_table.build_any(DbConnection::get_query_builder(&self.kind));
        let drop_table = sqlx::query(&drop_table);

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;

        transaction
            .execute(drop_table)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }

    pub(crate) async fn create_table(&self, ty: &ObjectType) -> anyhow::Result<()> {
        let mut create_table = Table::create()
            .table(Alias::new(&ty.backing_table))
            .if_not_exists()
            .col(
                ColumnDef::new(Alias::new("id"))
                    .integer()
                    .auto_increment()
                    .primary_key(),
            )
            .to_owned();
        for field in &ty.fields {
            let mut column_def = ColumnDef::new(Alias::new(&field.name));
            match field.type_ {
                Type::String => column_def.text(),
                Type::Int => column_def.integer(),
                Type::Float => column_def.double(),
                Type::Boolean => column_def.boolean(),
                Type::Object(_) => {
                    anyhow::bail!(QueryError::NotImplemented(
                        "support for type Object".to_owned(),
                    ));
                }
            };
            create_table.col(&mut column_def);
        }
        let create_table = create_table.build_any(DbConnection::get_query_builder(&self.kind));

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        let create_table = sqlx::query(&create_table);
        transaction
            .execute(create_table)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }

    pub(crate) fn query_relation(
        &self,
        rel: &Relation,
    ) -> Result<impl stream::Stream<Item = Result<AnyRow, Error>>, QueryError> {
        let query_str = sql(rel);
        Ok(QueryResults::new(query_str, &self.pool))
    }

    pub(crate) fn find_all(
        &self,
        ty: &ObjectType,
    ) -> anyhow::Result<impl stream::Stream<Item = Result<AnyRow, Error>>> {
        let query_str = format!("SELECT * FROM {}", ty.backing_table);
        Ok(QueryResults::new(query_str, &self.pool))
    }

    pub(crate) fn find_all_by(
        &self,
        ty: &ObjectType,
        field_name: &str,
        value_json: &serde_json::Value,
    ) -> anyhow::Result<impl stream::Stream<Item = Result<AnyRow, Error>>> {
        let key_field = ty
            .fields
            .iter()
            .find(|&f| f.name == field_name)
            .ok_or_else(|| QueryError::UnknownField(ty.name.clone(), field_name.to_string()))?;

        macro_rules! make_column_filter {
            ($as_type:ident) => {{
                let value = value_json.$as_type().ok_or_else(|| {
                    QueryError::IncompatibleData(key_field.name.to_owned(), ty.name.to_owned())
                })?;
                Expr::col(Alias::new(field_name)).eq(value)
            }};
        }
        let col_filter = match key_field.type_ {
            Type::String => make_column_filter!(as_str),
            Type::Int => make_column_filter!(as_i64),
            Type::Float => make_column_filter!(as_f64),
            Type::Boolean => make_column_filter!(as_bool),
            Type::Object(_) => {
                anyhow::bail!(QueryError::NotImplemented(
                    "support for type Object".to_owned(),
                ));
            }
        };

        let mut query = Query::select();
        for field in &ty.fields {
            query.column(Alias::new(&field.name));
        }
        let query = query
            .from(Alias::new(&ty.backing_table))
            .cond_where(col_filter)
            .to_owned();

        let query_str = match self.kind {
            Kind::Postgres => query.to_string(PostgresQueryBuilder),
            Kind::Sqlite => query.to_string(SqliteQueryBuilder),
        };
        Ok(QueryResults::new(query_str, &self.pool))
    }

    pub(crate) async fn add_row(
        &self,
        ty: &ObjectType,
        ty_value: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let insert_query = std::format!(
            "INSERT INTO {} ({}) VALUES ({})",
            &ty.backing_table,
            ty.fields.iter().map(|f| &f.name).join(", "),
            (0..ty.fields.len())
                .map(|i| std::format!("${}", i + 1))
                .join(", ")
        );

        let mut insert_query = sqlx::query(&insert_query);
        for field in &ty.fields {
            macro_rules! bind_default_json_value {
                (str, $value:expr) => {{
                    insert_query = insert_query.bind($value);
                }};
                ($fallback:ident, $value:expr) => {{
                    let value: $fallback = $value.as_str().parse().map_err(|_| {
                        QueryError::IncompatibleData(field.name.to_owned(), $value.clone())
                    })?;
                    insert_query = insert_query.bind(value);
                }};
            }

            macro_rules! bind_json_value {
                ($as_type:ident, $fallback:ident ) => {{
                    match ty_value.get(&field.name) {
                        Some(value_json) => {
                            let value = value_json.$as_type().ok_or_else(|| {
                                QueryError::IncompatibleData(
                                    field.name.to_owned(),
                                    ty.name.to_owned(),
                                )
                            })?;
                            insert_query = insert_query.bind(value);
                        }
                        None => {
                            let value = field.default.clone().ok_or_else(|| {
                                QueryError::IncompatibleData(
                                    field.name.to_owned(),
                                    ty.name.to_owned(),
                                )
                            })?;
                            bind_default_json_value!($fallback, value);
                        }
                    }
                }};
            }

            match field.type_ {
                Type::String => bind_json_value!(as_str, str),
                Type::Int => bind_json_value!(as_i64, i64),
                Type::Float => bind_json_value!(as_f64, f64),
                Type::Boolean => bind_json_value!(as_bool, bool),
                Type::Object(_) => {
                    anyhow::bail!(QueryError::NotImplemented(
                        "support for type Object".to_owned(),
                    ));
                }
            }
        }

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        transaction
            .execute(insert_query)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }
}

pub(crate) fn relational_row_to_json(
    relation: &Relation,
    row: &AnyRow,
) -> anyhow::Result<serde_json::Value> {
    let mut ret = json!({});
    for (query_column, result_column) in zip(&relation.columns, row.columns()) {
        let i = result_column.ordinal();
        // FIXME: consider result_column.type_info().is_null() too
        macro_rules! to_json {
            ($value_type:ty) => {{
                let val = row.get::<$value_type, _>(i);
                json!(val)
            }};
        }
        let val = match query_column.1 {
            Type::Float => to_json!(f64),
            Type::Int => to_json!(i64),
            Type::String => to_json!(&str),
            Type::Boolean => to_json!(bool),
            Type::Object(_) => unreachable!("A column cannot be a Type::Object"),
        };
        ret[result_column.name()] = val;
    }
    Ok(ret)
}

pub(crate) fn row_to_json(ty: &ObjectType, row: &AnyRow) -> anyhow::Result<serde_json::Value> {
    let mut v = serde_json::json!({});
    for field in &ty.fields {
        macro_rules! try_setting_field {
            ($value_type:ty) => {{
                let str_val = row
                    .try_get::<$value_type, _>(&*field.name)
                    .map_err(QueryError::ParsingFailed)?;
                v[&field.name] = serde_json::json!(str_val);
            }};
        }

        match field.type_ {
            Type::String => try_setting_field!(&str),
            Type::Int => try_setting_field!(i32),
            Type::Float => try_setting_field!(f64),
            Type::Boolean => try_setting_field!(bool),
            Type::Object(_) => {
                anyhow::bail!(QueryError::NotImplemented(
                    "support for type Object".to_owned(),
                ));
            }
        }
    }
    Ok(v)
}
