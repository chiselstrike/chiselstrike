// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::query::query_stream::QueryStream;
use crate::types::{ObjectType, TypeSystemError};
use futures::stream;
use sea_query::{Alias, ColumnDef, PostgresQueryBuilder, SchemaBuilder, SqliteQueryBuilder, Table};
use sqlx::any::{Any, AnyConnectOptions, AnyKind, AnyPool, AnyPoolOptions, AnyRow};
use sqlx::error::Error;
use sqlx::{Executor, Transaction};
use std::str::FromStr;

#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error["connection failed `{0}`"]]
    ConnectionFailed(#[source] sqlx::Error),
    #[error["execution failed: `{0}`"]]
    ExecuteFailed(#[source] sqlx::Error),
    #[error["fetch failed `{0}`"]]
    FetchFailed(#[source] sqlx::Error),
    #[error["type system error `{0}`"]]
    TypeError(#[from] TypeSystemError),
}

pub struct Store {
    data_opts: AnyConnectOptions,
    data_pool: AnyPool,
}

impl Store {
    pub fn new(data_opts: AnyConnectOptions, data_pool: AnyPool) -> Self {
        Self {
            data_opts,
            data_pool,
        }
    }

    pub async fn connect(data_uri: &str) -> Result<Self, StoreError> {
        let data_opts =
            AnyConnectOptions::from_str(data_uri).map_err(StoreError::ConnectionFailed)?;
        let data_pool = AnyPoolOptions::new()
            .connect(data_uri)
            .await
            .map_err(StoreError::ConnectionFailed)?;
        Ok(Store::new(data_opts, data_pool))
    }

    fn get_query_builder(opts: &AnyConnectOptions) -> &dyn SchemaBuilder {
        match opts.kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }

    pub async fn insert(&self, ty: ObjectType) -> Result<(), StoreError> {
        let mut transaction = self
            .data_pool
            .begin()
            .await
            .map_err(StoreError::ConnectionFailed)?;
        self.create_table(&ty, &mut transaction).await?;
        transaction
            .commit()
            .await
            .map_err(StoreError::ExecuteFailed)?;
        Ok(())
    }

    pub fn find_all(&self, ty: &ObjectType) -> impl stream::Stream<Item = Result<AnyRow, Error>> {
        let query_str = format!("SELECT * FROM {}", ty.backing_table);
        QueryStream::new(query_str, &self.data_pool)
    }

    async fn create_table(
        &self,
        ty: &ObjectType,
        transaction: &mut Transaction<'_, Any>,
    ) -> Result<(), StoreError> {
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
        let create_table = sqlx::query(&create_table);
        transaction
            .execute(create_table)
            .await
            .map_err(StoreError::ExecuteFailed)?;
        Ok(())
    }

    pub async fn add_row(&self, ty: &ObjectType, val: String) -> Result<(), StoreError> {
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
            .map_err(StoreError::ConnectionFailed)?;
        transaction
            .execute(insert_stmt)
            .await
            .map_err(StoreError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(StoreError::ExecuteFailed)?;
        Ok(())
    }
}
