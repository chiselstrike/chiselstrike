// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Context;
use anyhow::Result;
use sea_query::{PostgresQueryBuilder, QueryBuilder, SchemaBuilder, SqliteQueryBuilder};
use sqlx::any::{AnyKind, AnyPool, AnyPoolOptions};
use sqlx::Executor;

#[derive(Debug, Clone)]
pub struct DbConnection {
    pub pool: AnyPool,
}

impl DbConnection {
    pub async fn connect(uri: &str, max_connections: usize) -> Result<Self> {
        let pool = AnyPoolOptions::new()
            .max_connections(max_connections as u32)
            .after_connect(move |conn, _meta| {
                Box::pin(async move {
                    if matches!(conn.kind(), AnyKind::Sqlite) {
                        conn.execute("PRAGMA journal_mode=WAL;").await?;
                    }
                    Ok(())
                })
            })
            .connect(uri)
            .await
            .with_context(|| format!("failed to connect to {}", uri))?;
        Ok(Self { pool })
    }

    // TODO: replace `query_builder()` and `schema_builder()` with a single method that returns
    // `&dyn sea_query::GenericBulder`, once trait upcasting coercion is stabilized:
    // https://github.com/rust-lang/rust/issues/65991

    pub fn query_builder(&self) -> &'static dyn QueryBuilder {
        match self.pool.any_kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }

    pub fn schema_builder(&self) -> &'static dyn SchemaBuilder {
        match self.pool.any_kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }
}
