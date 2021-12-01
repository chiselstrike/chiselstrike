pub mod schema;

// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::query::{DbConnection, Kind, QueryError};
use crate::types::{Field, ObjectType, TypeSystem};
use sqlx::any::{Any, AnyPool};
use sqlx::{Executor, Row, Transaction};

/// Meta service.
///
/// The meta service is responsible for managing metadata such as object
/// types and labels persistently.
#[derive(Debug)]
pub struct MetaService {
    kind: Kind,
    pool: AnyPool,
}

impl MetaService {
    pub(crate) fn new(kind: Kind, pool: AnyPool) -> Self {
        Self { kind, pool }
    }

    pub(crate) async fn local_connection(conn: &DbConnection) -> Result<Self, QueryError> {
        let local = conn.local_connection().await?;
        Ok(Self::new(local.kind, local.pool))
    }

    /// Create the schema of the underlying metadata store.
    pub(crate) async fn create_schema(&self) -> Result<(), QueryError> {
        let query_builder = DbConnection::get_query_builder(&self.kind);
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

    /// Load the type system from metadata store.
    pub(crate) async fn load_type_system<'r>(&self) -> Result<TypeSystem, QueryError> {
        let query = sqlx::query("SELECT types.type_id AS type_id, types.api_version AS api_version, type_names.name AS type_name FROM types INNER JOIN type_names WHERE types.type_id = type_names.type_id");
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(QueryError::FetchFailed)?;
        let mut ts = TypeSystem::new();
        for row in rows {
            let type_id: i32 = row.get("type_id");
            let api_version: &str = row.get("api_version");

            let type_name: &str = row.get("type_name");
            let split: Vec<&str> = type_name.split('.').collect();
            let version = split[0];
            assert_eq!(
                version, api_version,
                "expected api version {} doesn't match what's in the database {}. Corrupted?",
                api_version, version
            );
            let type_name = split[1];

            let fields = self.load_type_fields(&ts, type_id, api_version).await?;
            ts.add_type(ObjectType::new(type_name, fields, api_version))?;
        }
        Ok(ts)
    }

    async fn load_type_fields(
        &self,
        ts: &TypeSystem,
        type_id: i32,
        api_version: &str,
    ) -> Result<Vec<Field>, QueryError> {
        let query = sqlx::query("SELECT fields.field_id AS field_id, field_names.field_name AS field_name, fields.field_type AS field_type, fields.default_value as default_value, fields.is_optional as is_optional FROM field_names INNER JOIN fields WHERE fields.type_id = $1 AND field_names.field_id = fields.field_id;");
        let query = query.bind(type_id);
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(QueryError::FetchFailed)?;
        let mut fields = Vec::new();
        for row in rows {
            let field_name: &str = row.get("field_name");
            let split: Vec<&str> = field_name.split('.').collect();
            let version = split[0];
            assert_eq!(
                version, api_version,
                "expected api version {} doesn't match what's in the database {}. Corrupted?",
                api_version, version
            );
            let field_name = split[2];

            let field_type: &str = row.get("field_type");
            let field_id: i32 = row.get("field_id");
            let field_def: Option<String> = row.get("default_value");
            let is_optional: bool = row.get("is_optional");
            let ty = ts.lookup_type(field_type, api_version)?;
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
                default: field_def,
                is_optional,
            });
        }
        Ok(fields)
    }

    pub(crate) async fn insert(&self, ty: ObjectType) -> Result<(), QueryError> {
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
        let add_type = sqlx::query("INSERT INTO types (api_version) VALUES ($1) RETURNING *");
        let add_type_name = sqlx::query("INSERT INTO type_names (type_id, name) VALUES ($1, $2)");

        let add_type = add_type.bind(ty.api_version.clone());
        let row = transaction
            .fetch_one(add_type)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        let id: i32 = row.get("type_id");

        let full_name = format!("{}.{}", &ty.api_version, &ty.name);
        let add_type_name = add_type_name.bind(id).bind(full_name);
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
            let full_name = format!("{}.{}.{}", &ty.api_version, &ty.name, &field.name);
            let add_field_name = add_field_name.bind(full_name).bind(field_id);
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
}
