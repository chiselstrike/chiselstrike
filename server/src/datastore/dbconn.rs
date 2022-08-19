// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Context;
use anyhow::Result;
use sea_query::{PostgresQueryBuilder, SchemaBuilder, SqliteQueryBuilder};
use sqlx::any::{AnyKind, AnyPool, AnyPoolOptions};
use sqlx::Executor;

#[derive(Debug, Clone)]
pub struct DbConnection {
    pub pool: AnyPool,
    pub conn_uri: String,
}

impl DbConnection {
    pub async fn connect(uri: &str, nr_conn: usize) -> Result<Self> {
        let pool = AnyPoolOptions::new()
            .max_connections(nr_conn as _)
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

        let conn_uri = uri.to_owned();

        Ok(Self { pool, conn_uri })
    }

    pub async fn local_connection(&self, nr_conn: usize) -> Result<Self> {
        match self.pool.any_kind() {
            AnyKind::Postgres => Self::connect(&self.conn_uri, nr_conn).await,
            AnyKind::Sqlite => Ok(self.clone()),
        }
    }

    pub fn query_builder(&self) -> &dyn SchemaBuilder {
        match self.pool.any_kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }
}
