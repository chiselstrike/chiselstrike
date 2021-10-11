// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::types::{ObjectType, Type, TypeSystem, TypeSystemError};
use sea_query::{
    ColumnDef, Iden, PostgresQueryBuilder, SchemaBuilder, SqliteQueryBuilder, Table,
};
use sqlx::any::{Any, AnyConnectOptions, AnyKind, AnyPool, AnyPoolOptions};
use sqlx::{Executor, Row, Transaction};
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

#[derive(Iden)]
enum Types {
    Table,
    TypeId,
    BackingTable,
}

pub struct Store {
    opts: AnyConnectOptions,
    pool: AnyPool,
    data_opts: AnyConnectOptions,
    data_pool: AnyPool,
}

impl Store {
    pub fn new(
        opts: AnyConnectOptions,
        pool: AnyPool,
        data_opts: AnyConnectOptions,
        data_pool: AnyPool,
    ) -> Self {
        Self {
            opts,
            pool,
            data_opts,
            data_pool,
        }
    }

    pub async fn connect(meta_uri: &str, data_uri: &str) -> Result<Self, StoreError> {
        let opts = AnyConnectOptions::from_str(meta_uri).map_err(StoreError::ConnectionFailed)?;
        let pool = AnyPoolOptions::new()
            .connect(meta_uri)
            .await
            .map_err(StoreError::ConnectionFailed)?;
        let data_opts =
            AnyConnectOptions::from_str(data_uri).map_err(StoreError::ConnectionFailed)?;
        let data_pool = AnyPoolOptions::new()
            .connect(data_uri)
            .await
            .map_err(StoreError::ConnectionFailed)?;
        Ok(Store::new(opts, pool, data_opts, data_pool))
    }

    /// Create the schema of the underlying metadata store.
    pub async fn create_schema(&self) -> Result<(), StoreError> {
        let create_types = Table::create()
            .table(Types::Table)
            .if_not_exists()
            .col(
                ColumnDef::new(Types::TypeId)
                    .integer()
                    .auto_increment()
                    .primary_key(),
            )
            .col(ColumnDef::new(Types::BackingTable).text().unique_key())
            .build_any(Self::get_query_builder(&self.opts));
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

    fn get_query_builder(opts: &AnyConnectOptions) -> &dyn SchemaBuilder {
        match opts.kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }

    fn primary_key_sql(kind: AnyKind) -> &'static str {
        match kind {
            AnyKind::Postgres => "SERIAL PRIMARY KEY",
            AnyKind::Sqlite => "INTEGER PRIMARY KEY AUTOINCREMENT",
        }
    }

    /// Load the type system from metadata store.
    pub async fn load_type_system<'r>(&self) -> Result<TypeSystem, StoreError> {
        let query = sqlx::query("SELECT types.type_id AS type_id, types.backing_table AS backing_table, type_names.name AS type_name FROM types INNER JOIN type_names WHERE types.type_id = type_names.type_id");
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::FetchFailed)?;
        let mut ts = TypeSystem::new();
        for row in rows {
            let type_id: i32 = row.get("type_id");
            let backing_table: &str = row.get("backing_table");
            let type_name: &str = row.get("type_name");
            let fields = self.load_type_fields(&ts, type_id).await?;
            ts.define_type(ObjectType {
                name: type_name.to_string(),
                fields,
                backing_table: backing_table.to_string(),
            })?;
        }
        Ok(ts)
    }

    async fn load_type_fields(
        &self,
        ts: &TypeSystem,
        type_id: i32,
    ) -> Result<Vec<(String, Type)>, StoreError> {
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
        Ok(fields)
    }

    pub async fn insert(&self, ty: ObjectType) -> Result<(), StoreError> {
        // FIXME: Multi-database transaction is needed for consistency.
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(StoreError::ConnectionFailed)?;
        self.insert_type(&ty, &mut transaction).await?;
        transaction
            .commit()
            .await
            .map_err(StoreError::ExecuteFailed)?;
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

    async fn insert_type(
        &self,
        ty: &ObjectType,
        transaction: &mut Transaction<'_, Any>,
    ) -> Result<(), StoreError> {
        let add_type = sqlx::query("INSERT INTO types (backing_table) VALUES ($1) RETURNING *");
        let add_type_name = sqlx::query("INSERT INTO type_names (type_id, name) VALUES ($1, $2)");

        let add_type = add_type.bind(ty.backing_table.clone());
        let row = transaction
            .fetch_one(add_type)
            .await
            .map_err(StoreError::ExecuteFailed)?;
        let id: i32 = row.get("type_id");
        let add_type_name = add_type_name.bind(id).bind(ty.name.clone());
        transaction
            .execute(add_type_name)
            .await
            .map_err(StoreError::ExecuteFailed)?;
        for (field_name, field_type) in &ty.fields {
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
        Ok(())
    }

    async fn create_table(
        &self,
        ty: &ObjectType,
        transaction: &mut Transaction<'_, Any>,
    ) -> Result<(), StoreError> {
        let create_table = format!(
            "CREATE TABLE IF NOT EXISTS {} (id {}, fields TEXT)",
            ty.backing_table.clone(),
            Store::primary_key_sql(self.data_opts.kind())
        );
        let create_table = sqlx::query(&create_table);
        transaction
            .execute(create_table)
            .await
            .map_err(StoreError::ExecuteFailed)?;
        Ok(())
    }
}
