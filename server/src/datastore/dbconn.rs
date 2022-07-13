// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Context;
use anyhow::Result;
use sea_query::{PostgresQueryBuilder, SchemaBuilder, SqliteQueryBuilder};
use sqlx::any::{AnyConnectOptions, AnyKind, AnyPool, AnyPoolOptions};
use std::str::FromStr;

// FIXME: Sqlite's Anykind does not implement Copy / Clone. It got merged
// in their cdb40b1f8e5f, but that was not released yet. So temporarily wrap
// around ours. When they release we can remove this.
#[derive(Debug, Copy, Clone)]
pub(crate) enum Kind {
    Postgres,
    Sqlite,
}

impl From<Kind> for AnyKind {
    fn from(k: Kind) -> AnyKind {
        match k {
            Kind::Sqlite => AnyKind::Sqlite,
            Kind::Postgres => AnyKind::Postgres,
        }
    }
}

impl From<AnyKind> for Kind {
    fn from(k: AnyKind) -> Kind {
        match k {
            AnyKind::Sqlite => Kind::Sqlite,
            AnyKind::Postgres => Kind::Postgres,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DbConnection {
    pub(crate) kind: Kind,
    pub(crate) pool: AnyPool,
    pub(crate) conn_uri: String,
}

impl DbConnection {
    pub(crate) async fn connect(uri: &str, nr_conn: usize) -> Result<Self> {
        let opts = AnyConnectOptions::from_str(uri)?;
        let pool = AnyPoolOptions::new()
            .max_connections(nr_conn as _)
            .connect(uri)
            .await
            .with_context(|| format!("failed to connect to {}", uri))?;

        let conn_uri = uri.to_owned();

        Ok(Self {
            kind: opts.kind().into(),
            pool,
            conn_uri,
        })
    }

    pub(crate) async fn local_connection(&self, nr_conn: usize) -> Result<Self> {
        match self.kind {
            Kind::Postgres => Self::connect(&self.conn_uri, nr_conn).await,
            Kind::Sqlite => Ok(self.clone()),
        }
    }

    pub(crate) fn get_query_builder(kind: &Kind) -> &dyn SchemaBuilder {
        match kind {
            Kind::Postgres => &PostgresQueryBuilder,
            Kind::Sqlite => &SqliteQueryBuilder,
        }
    }
}
