use crate::types::{Type, TypeSystem, TypeSystemError};
use sqlx::any::{AnyConnectOptions, AnyKind, AnyPool, AnyPoolOptions};
use sqlx::Row;
use std::str::FromStr;

#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error["connection failed"]]
    ConnectionFailed(#[source] sqlx::Error),
    #[error["execution failed"]]
    ExecuteFailed(#[source] sqlx::Error),
    #[error["fetch failed"]]
    FetchFailed(#[source] sqlx::Error),
    #[error["type system error"]]
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
        let query = match self.opts.kind() {
            AnyKind::Postgres => {
                "CREATE TABLE IF NOT EXISTS types (
                        type_id SERIAL PRIMARY KEY,
                        name TEXT
                    )"
            }
            AnyKind::Sqlite => {
                "CREATE TABLE IF NOT EXISTS types (
                    type_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT
                )"
            }
        };
        let query = sqlx::query(query);
        query
            .execute(&self.pool)
            .await
            .map_err(StoreError::ConnectionFailed)?;
        Ok(())
    }

    pub async fn load_schema<'r>(&self) -> Result<TypeSystem, StoreError> {
        let query = sqlx::query("SELECT name FROM types");
        let types = query
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::FetchFailed)?;
        let mut ts = TypeSystem::new();
        for ty in types {
            let name: String = ty.get(0);
            ts.define_type(Type { name })?;
        }
        Ok(ts)
    }

    pub async fn insert(&self, ty: Type) -> Result<(), StoreError> {
        let query = sqlx::query("INSERT INTO types (name) VALUES ($1)").bind(ty.name);
        query
            .execute(&self.pool)
            .await
            .map_err(StoreError::ConnectionFailed)?;
        Ok(())
    }
}
