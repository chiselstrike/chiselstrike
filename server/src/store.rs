use crate::types::{Type, TypeSystem};
use sqlx::any::{AnyPool, AnyPoolOptions};
use sqlx::Row;

#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error["connection failed"]]
    ConnectionFailed(#[source] sqlx::Error),
    #[error["execution failed"]]
    ExecuteFailed(#[source] sqlx::Error),
    #[error["fetch failed"]]
    FetchFailed(#[source] sqlx::Error),
}

pub struct Store {
    pool: AnyPool,
}

impl Store {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn connect(uri: &str) -> Result<Self, StoreError> {
        let pool = AnyPoolOptions::new()
            .connect(uri)
            .await
            .map_err(StoreError::ConnectionFailed)?;
        Ok(Store::new(pool))
    }

    pub async fn create_schema(&self) -> Result<(), StoreError> {
        let query = sqlx::query("CREATE TABLE IF NOT EXISTS types (name TEXT)");
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
            ts.define_type(Type { name });
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
