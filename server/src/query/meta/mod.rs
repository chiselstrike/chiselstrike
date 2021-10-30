pub mod schema;

// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::query::QueryError;
use crate::types::{Field, ObjectType, TypeSystem};
use sea_query::{PostgresQueryBuilder, SchemaBuilder, SqliteQueryBuilder};
use sqlx::any::{Any, AnyConnectOptions, AnyKind, AnyPool, AnyPoolOptions};
use sqlx::{Executor, Row, Transaction};
use std::str::FromStr;

pub struct MetaService {
    opts: AnyConnectOptions,
    pool: AnyPool,
}

impl MetaService {
    pub fn new(opts: AnyConnectOptions, pool: AnyPool) -> Self {
        Self { opts, pool }
    }

    pub async fn connect(meta_uri: &str) -> Result<Self, QueryError> {
        let opts = AnyConnectOptions::from_str(meta_uri).map_err(QueryError::ConnectionFailed)?;
        let pool = AnyPoolOptions::new()
            .connect(meta_uri)
            .await
            .map_err(QueryError::ConnectionFailed)?;
        Ok(MetaService { opts, pool })
    }

    /// Create the schema of the underlying metadata store.
    pub async fn create_schema(&self) -> Result<(), QueryError> {
        let query_builder = Self::get_query_builder(&self.opts);
        let tables = schema::tables();
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        for table in tables {
            let query = table.build_any(query_builder);
            let query = sqlx::query(&query);
            conn.execute(query)
                .await
                .map_err(QueryError::ExecuteFailed)?;
        }
        Ok(())
    }

    fn get_query_builder(opts: &AnyConnectOptions) -> &dyn SchemaBuilder {
        match opts.kind() {
            AnyKind::Postgres => &PostgresQueryBuilder,
            AnyKind::Sqlite => &SqliteQueryBuilder,
        }
    }

    /// Load the type system from metadata store.
    pub async fn load_type_system<'r>(&self) -> Result<TypeSystem, QueryError> {
        let query = sqlx::query("SELECT types.type_id AS type_id, types.backing_table AS backing_table, type_names.name AS type_name FROM types INNER JOIN type_names WHERE types.type_id = type_names.type_id");
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(QueryError::FetchFailed)?;
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
    ) -> Result<Vec<Field>, QueryError> {
        let query = sqlx::query("SELECT fields.field_id AS field_id, field_names.field_name AS field_name, fields.field_type AS field_type FROM field_names INNER JOIN fields WHERE fields.type_id = $1 AND field_names.field_id = fields.field_id;");
        let query = query.bind(type_id);
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(QueryError::FetchFailed)?;
        let mut fields = Vec::new();
        for row in rows {
            let field_name: &str = row.get("field_name");
            let field_type: &str = row.get("field_type");
            let field_id: i32 = row.get("field_id");
            let ty = ts.lookup_type(field_type)?;
            let labels_query =
                sqlx::query("SELECT label_name FROM field_labels WHERE field_id = $1");
            let labels = labels_query
                .bind(field_id)
                .fetch_all(&self.pool)
                .await
                .map_err(QueryError::FetchFailed)?
                .iter()
                .map(|r| r.get("label_name"))
                .collect::<Vec<String>>();
            fields.push(Field {
                name: field_name.to_string(),
                type_: ty,
                labels,
            });
        }
        Ok(fields)
    }

    pub async fn insert(&self, ty: ObjectType) -> Result<(), QueryError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        self.insert_type(&ty, &mut transaction).await?;
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }

    async fn insert_type(
        &self,
        ty: &ObjectType,
        transaction: &mut Transaction<'_, Any>,
    ) -> Result<(), QueryError> {
        let add_type = sqlx::query("INSERT INTO types (backing_table) VALUES ($1) RETURNING *");
        let add_type_name = sqlx::query("INSERT INTO type_names (type_id, name) VALUES ($1, $2)");

        let add_type = add_type.bind(ty.backing_table.clone());
        let row = transaction
            .fetch_one(add_type)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        let id: i32 = row.get("type_id");
        let add_type_name = add_type_name.bind(id).bind(ty.name.clone());
        transaction
            .execute(add_type_name)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        for field in &ty.fields {
            let add_field =
                sqlx::query("INSERT INTO fields (field_type, type_id) VALUES ($1, $2) RETURNING *");
            let add_field_name =
                sqlx::query("INSERT INTO field_names (field_name, field_id) VALUES ($1, $2)");
            let add_field = add_field.bind(field.type_.name()).bind(id);
            let row = transaction
                .fetch_one(add_field)
                .await
                .map_err(QueryError::ExecuteFailed)?;
            let field_id: i32 = row.get("field_id");
            let add_field_name = add_field_name.bind(field.name.clone()).bind(field_id);
            transaction
                .execute(add_field_name)
                .await
                .map_err(QueryError::ExecuteFailed)?;
            for label in &field.labels {
                let q =
                    sqlx::query("INSERT INTO field_labels (label_name, field_id) VALUES ($1, $2)");
                transaction
                    .execute(q.bind(label).bind(field_id))
                    .await
                    .map_err(QueryError::ExecuteFailed)?;
            }
        }
        Ok(())
    }

    pub async fn remove(&self, type_name: &str) -> Result<(), QueryError> {
        let delete_type = sqlx::query(
            "DELETE FROM types WHERE type_id = (SELECT type_id FROM type_names WHERE name = $1)",
        );
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        let delete_type = delete_type.bind(type_name);
        transaction
            .execute(delete_type)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }
}
