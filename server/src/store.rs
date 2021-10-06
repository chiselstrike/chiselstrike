// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::types::{Type, TypeSystem, TypeSystemError};
use sqlx::any::{AnyConnectOptions, AnyKind, AnyPool, AnyPoolOptions};
use sqlx::Executor;
use sqlx::Row;
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
    opts: AnyConnectOptions,
    pool: AnyPool,
}

impl Store {
    pub fn new(opts: AnyConnectOptions, pool: AnyPool) -> Self {
        Self { opts, pool }
    }

    pub async fn connect(uri: &str) -> Result<Self, StoreError> {
        let opts = AnyConnectOptions::from_str(uri).map_err(StoreError::ConnectionFailed)?;
        let pool = AnyPoolOptions::new()
            .connect(uri)
            .await
            .map_err(StoreError::ConnectionFailed)?;
        Ok(Store::new(opts, pool))
    }

    pub async fn create_schema(&self) -> Result<(), StoreError> {
        let create_types = format!(
            "CREATE TABLE IF NOT EXISTS types (type_id {})",
            Store::primary_key_sql(self.opts.kind())
        );
        let create_types = sqlx::query(&create_types);
        let create_type_names = sqlx::query(
            "CREATE TABLE IF NOT EXISTS type_names (
                 type_id INTEGER REFERENCES types(type_id),
                 name TEXT UNIQUE
             )",
        );
        let queries = vec![create_types, create_type_names];
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(StoreError::ConnectionFailed)?;
        for query in queries {
            conn.execute(query)
                .await
                .map_err(StoreError::ExecuteFailed)?;
        }
        Ok(())
    }

    fn primary_key_sql(kind: AnyKind) -> &'static str {
        match kind {
            AnyKind::Postgres => "SERIAL PRIMARY KEY",
            AnyKind::Sqlite => "INTEGER PRIMARY KEY AUTOINCREMENT",
        }
    }

    pub async fn load_schema<'r>(&self) -> Result<TypeSystem, StoreError> {
        let query = sqlx::query("SELECT name FROM type_names");
        let types = query
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::FetchFailed)?;
        let mut ts = TypeSystem::new();
        for ty in types {
            let name: String = ty.get(0);
            ts.define_type(Type {
                name,
                fields: Vec::default(),
            })?;
        }
        Ok(ts)
    }

    pub async fn insert(&self, ty: Type) -> Result<(), StoreError> {
        let add_type = sqlx::query("INSERT INTO types DEFAULT VALUES RETURNING *");
        let add_type_name = sqlx::query("INSERT INTO type_names (type_id, name) VALUES ($1, $2)");

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(StoreError::ConnectionFailed)?;
        let row = transaction
            .fetch_one(add_type)
            .await
            .map_err(StoreError::ExecuteFailed)?;
        let id: i32 = row.get("type_id");
        let add_type_name = add_type_name.bind(id).bind(ty.name);
        transaction
            .execute(add_type_name)
            .await
            .map_err(StoreError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(StoreError::ExecuteFailed)?;
        Ok(())
    }
}
