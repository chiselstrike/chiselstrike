// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::query::QueryError;
use crate::types::{ObjectType, Type};
use anyhow::Result;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use itertools::Itertools;
use sea_query::{Alias, ColumnDef, PostgresQueryBuilder, SchemaBuilder, SqliteQueryBuilder, Table};
use sqlx::any::{Any, AnyConnectOptions, AnyKind, AnyPool, AnyPoolOptions, AnyRow};
use sqlx::error::Error;
use sqlx::{Executor, Row};
use std::cell::RefCell;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};

pub struct QueryResults<'a> {
    raw_query: String,
    stream: RefCell<Option<BoxStream<'a, Result<AnyRow, Error>>>>,
    _marker: PhantomPinned, // QueryStream cannot be moved
}

impl<'a> QueryResults<'a> {
    pub fn new(raw_query: String, pool: &AnyPool) -> Pin<Box<Self>> {
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

impl Stream for QueryResults<'_> {
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
pub struct QueryEngine {
    opts: AnyConnectOptions,
    pool: AnyPool,
}

impl QueryEngine {
    pub fn new(opts: AnyConnectOptions, pool: AnyPool) -> Self {
        Self { opts, pool }
    }

    pub async fn connect(uri: &str) -> Result<Self, QueryError> {
        let opts = AnyConnectOptions::from_str(uri).map_err(QueryError::ConnectionFailed)?;
        let pool = AnyPoolOptions::new()
            .connect(uri)
            .await
            .map_err(QueryError::ConnectionFailed)?;
        Ok(QueryEngine::new(opts, pool))
    }

    fn get_query_builder(opts: &AnyConnectOptions) -> &dyn SchemaBuilder {
        match opts.kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }

    pub fn find_all(&self, ty: &ObjectType) -> impl stream::Stream<Item = Result<AnyRow, Error>> {
        let query_str = format!("SELECT * FROM {}", ty.backing_table);
        QueryResults::new(query_str, &self.pool)
    }

    pub async fn create_table(&self, ty: ObjectType) -> Result<(), QueryError> {
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
        for field in ty.fields {
            let mut column_def = ColumnDef::new(Alias::new(&field.name));
            match field.type_ {
                Type::String => column_def.text(),
                Type::Int => column_def.integer(),
                Type::Float => column_def.double(),
                Type::Boolean => column_def.boolean(),
                Type::Object(_) => {
                    return Err(QueryError::NotImplemented(
                        "support for type Object".to_owned(),
                    ));
                }
            };
            create_table.col(&mut column_def);
        }
        let create_table = create_table.build_any(Self::get_query_builder(&self.opts));

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

    pub async fn add_row(
        &self,
        ty: &ObjectType,
        ty_value: &serde_json::Value,
    ) -> Result<(), QueryError> {
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
            macro_rules! bind_json_value {
                ($as_type:ident) => {{
                    let value_json = ty_value.get(&field.name).ok_or_else(|| {
                        QueryError::IncompatibleData(field.name.to_owned(), ty.name.to_owned())
                    })?;
                    let value = value_json.$as_type().ok_or_else(|| {
                        QueryError::IncompatibleData(field.name.to_owned(), ty.name.to_owned())
                    })?;
                    insert_query = insert_query.bind(value);
                }};
            }

            match field.type_ {
                Type::String => bind_json_value!(as_str),
                Type::Int => bind_json_value!(as_i64),
                Type::Float => bind_json_value!(as_f64),
                Type::Boolean => bind_json_value!(as_bool),
                Type::Object(_) => {
                    return Err(QueryError::NotImplemented(
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

pub fn row_to_json(ty: &ObjectType, row: &AnyRow) -> Result<serde_json::Value, QueryError> {
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
                return Err(QueryError::NotImplemented(
                    "support for type Object".to_owned(),
                ));
            }
        }
    }
    Ok(v)
}
