// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Context;
use anyhow::Result;
use sea_query::{PostgresQueryBuilder, SchemaBuilder, SqliteQueryBuilder};
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

    pub fn query_builder(&self) -> &dyn SchemaBuilder {
        match self.pool.any_kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }
}
