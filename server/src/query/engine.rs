// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::query::QueryError;
use crate::types::ObjectType;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use sea_query::{Alias, ColumnDef, PostgresQueryBuilder, SchemaBuilder, SqliteQueryBuilder, Table};
use sqlx::any::{Any, AnyConnectOptions, AnyKind, AnyPool, AnyPoolOptions, AnyRow};
use sqlx::error::Error;
use sqlx::Executor;
use std::cell::RefCell;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};

pub struct QueryStream<'a> {
    raw_query: String,
    stream: RefCell<Option<BoxStream<'a, Result<AnyRow, Error>>>>,
    _marker: PhantomPinned, // QueryStream cannot be moved
}

impl<'a> QueryStream<'a> {
    pub fn new(raw_query: String, pool: &AnyPool) -> Pin<Box<Self>> {
        let ret = Box::pin(QueryStream {
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

impl Stream for QueryStream<'_> {
    type Item = Result<AnyRow, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut borrow = self.stream.borrow_mut();
        borrow.as_mut().unwrap().as_mut().poll_next(cx)
    }
}

pub struct QueryEngine {
    data_opts: AnyConnectOptions,
    data_pool: AnyPool,
}

impl QueryEngine {
    pub fn new(data_opts: AnyConnectOptions, data_pool: AnyPool) -> Self {
        Self {
            data_opts,
            data_pool,
        }
    }

    pub async fn connect(data_uri: &str) -> Result<Self, QueryError> {
        let data_opts =
            AnyConnectOptions::from_str(data_uri).map_err(QueryError::ConnectionFailed)?;
        let data_pool = AnyPoolOptions::new()
            .connect(data_uri)
            .await
            .map_err(QueryError::ConnectionFailed)?;
        Ok(QueryEngine::new(data_opts, data_pool))
    }

    fn get_query_builder(opts: &AnyConnectOptions) -> &dyn SchemaBuilder {
        match opts.kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }

    pub fn find_all(&self, ty: &ObjectType) -> impl stream::Stream<Item = Result<AnyRow, Error>> {
        let query_str = format!("SELECT * FROM {}", ty.backing_table);
        QueryStream::new(query_str, &self.data_pool)
    }

    pub async fn create_table(&self, ty: ObjectType) -> Result<(), QueryError> {
        let create_table = Table::create()
            .table(Alias::new(&ty.backing_table))
            .if_not_exists()
            .col(
                ColumnDef::new(Alias::new("id"))
                    .integer()
                    .auto_increment()
                    .primary_key(),
            )
            .col(ColumnDef::new(Alias::new("fields")).text())
            .build_any(Self::get_query_builder(&self.data_opts));
        let mut transaction = self
            .data_pool
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

    pub async fn add_row(&self, ty: &ObjectType, val: String) -> Result<(), QueryError> {
        // TODO: escape quotes in val where necessary.
        let query = format!(
            "INSERT INTO {}(fields) VALUES ('{}')",
            ty.backing_table, val
        );
        let insert_stmt = sqlx::query(&query);
        let mut transaction = self
            .data_pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        transaction
            .execute(insert_stmt)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }
}
