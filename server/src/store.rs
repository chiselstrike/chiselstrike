// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::types::{ObjectType, TypeSystem, TypeSystemError};
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
        let create_type_names = "CREATE TABLE IF NOT EXISTS type_names (
                 type_id INTEGER REFERENCES types(type_id),
                 name TEXT UNIQUE
             )"
        .to_string();
        let create_fields = format!(
            "CREATE TABLE IF NOT EXISTS fields (
                field_id {},
                field_type TEXT,
                type_id INTEGER REFERENCES types(type_id)
            )",
            Store::primary_key_sql(self.opts.kind())
        );
        let create_type_fields = "CREATE TABLE IF NOT EXISTS field_names (
                field_name TEXT UNIQUE,
                field_id INTEGER REFERENCES fields(field_id)
            )"
        .to_string();
        let queries = vec![
            create_types,
            create_type_names,
            create_fields,
            create_type_fields,
        ];
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(StoreError::ConnectionFailed)?;
        for query in queries {
            let query = sqlx::query(&query);
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
        let query = sqlx::query("SELECT types.type_id AS type_id, type_names.name AS type_name FROM types INNER JOIN type_names WHERE types.type_id = type_names.type_id");
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::FetchFailed)?;
        let mut ts = TypeSystem::new();
        for row in rows {
            let type_id: i32 = row.get("type_id");
            let type_name: &str = row.get("type_name");
            let query = sqlx::query("SELECT field_names.field_name AS field_name, fields.field_type AS field_type FROM field_names INNER JOIN fields WHERE fields.type_id = $1 AND field_names.field_id = fields.field_id;");
            let query = query.bind(type_id);
            let rows = query
                .fetch_all(&self.pool)
                .await
                .map_err(StoreError::FetchFailed)?;
            let mut fields = Vec::new();
            for row in rows {
                let field_name: &str = row.get("field_name");
                let field_type: &str = row.get("field_type");
                let ty = ts.lookup_type(field_type)?;
                fields.push((field_name.to_string(), ty));
            }
            ts.define_type(ObjectType {
                name: type_name.to_string(),
                fields,
            })?;
        }
        Ok(ts)
    }

    pub async fn insert(&self, ty: ObjectType) -> Result<(), StoreError> {
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
        for (field_name, field_type) in ty.fields {
            let add_field =
                sqlx::query("INSERT INTO fields (field_type, type_id) VALUES ($1, $2) RETURNING *");
            let add_field_name =
                sqlx::query("INSERT INTO field_names (field_name, field_id) VALUES ($1, $2)");
            let add_field = add_field.bind(field_type.name()).bind(id);
            let row = transaction
                .fetch_one(add_field)
                .await
                .map_err(StoreError::ExecuteFailed)?;
            let field_id: i32 = row.get("field_id");
            let add_field_name = add_field_name.bind(field_name).bind(field_id);
            transaction
                .execute(add_field_name)
                .await
                .map_err(StoreError::ExecuteFailed)?;
        }
        transaction
            .commit()
            .await
            .map_err(StoreError::ExecuteFailed)?;
        Ok(())
    }
}
