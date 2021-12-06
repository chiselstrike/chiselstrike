pub(crate) mod schema;

// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::RoutePaths;
use crate::deno;
use crate::query::{DbConnection, Kind, QueryError};
use crate::types::{Field, FieldDelta, ObjectDelta, ObjectType, TypeSystem};
use anyhow::anyhow;
use futures::FutureExt;
use sqlx::any::{Any, AnyPool};
use sqlx::{Executor, Row, Transaction};
use std::sync::Arc;
use uuid::Uuid;

/// Meta service.
///
/// The meta service is responsible for managing metadata such as object
/// types and labels persistently.
#[derive(Debug)]
pub(crate) struct MetaService {
    kind: Kind,
    pool: AnyPool,
}

async fn update_field_query(
    transaction: &mut Transaction<'_, Any>,
    delta: &FieldDelta,
) -> anyhow::Result<()> {
    let field_id = delta.id;

    if let Some(field) = &delta.attrs {
        let default_stmt = if field.default.is_none() {
            ""
        } else {
            ", default_value = $4"
        };

        let querystr = format!(
            "UPDATE fields SET field_type = $1, is_optional = $2 {} WHERE field_id = $3",
            default_stmt
        );
        let mut query = sqlx::query(&querystr);

        query = query
            .bind(field.type_.name())
            .bind(field.is_optional)
            .bind(field_id);

        if let Some(value) = &field.default {
            query = query.bind(value.to_owned());
        }

        transaction
            .execute(query)
            .await
            .map_err(QueryError::ExecuteFailed)?;
    }

    if let Some(labels) = &delta.labels {
        let flush = sqlx::query("DELETE FROM field_labels where field_id = $1");
        transaction
            .execute(flush.bind(field_id))
            .await
            .map_err(QueryError::ExecuteFailed)?;

        for label in labels.iter() {
            let q = sqlx::query("INSERT INTO field_labels (label_name, field_id) VALUES ($1, $2)");
            transaction
                .execute(q.bind(label).bind(field_id))
                .await
                .map_err(QueryError::ExecuteFailed)?;
        }
    }
    Ok(())
}

async fn remove_field_query(
    transaction: &mut Transaction<'_, Any>,
    field: &Field,
) -> anyhow::Result<()> {
    let field_id = field
        .id
        .ok_or_else(|| anyhow!("logical error. Trying to delete field without id"))?;

    let query = sqlx::query("DELETE FROM fields WHERE field_id = $1");
    transaction
        .execute(query.bind(field_id))
        .await
        .map_err(QueryError::ExecuteFailed)?;

    let query = sqlx::query("DELETE from field_names WHERE field_id = $1");
    transaction
        .execute(query.bind(field_id))
        .await
        .map_err(QueryError::ExecuteFailed)?;

    let query = sqlx::query("DELETE from field_labels WHERE field_id = $1");
    transaction
        .execute(query.bind(field_id))
        .await
        .map_err(QueryError::ExecuteFailed)?;

    Ok(())
}

async fn insert_field_query(
    transaction: &mut Transaction<'_, Any>,
    ty: &ObjectType,
    recently_added_type_id: Option<i32>,
    field: &Field,
) -> anyhow::Result<()> {
    let type_id = ty.id.xor(recently_added_type_id).ok_or_else(|| anyhow!("logical error. Seems like a type is at the same type pre-existing and recently added??"))?;

    let add_field = match &field.default {
        None => {
            let query = sqlx::query("INSERT INTO fields (field_type, type_id, is_optional) VALUES ($1, $2, $3) RETURNING *");
            query
                .bind(field.type_.name())
                .bind(type_id)
                .bind(field.is_optional)
        }
        Some(value) => {
            let query = sqlx::query("INSERT INTO fields (field_type, type_id, default_value, is_optional) VALUES ($1, $2, $3, $4) RETURNING *");
            query
                .bind(field.type_.name())
                .bind(type_id)
                .bind(value.to_owned())
                .bind(field.is_optional)
        }
    };
    let add_field_name =
        sqlx::query("INSERT INTO field_names (field_name, field_id) VALUES ($1, $2)");

    let row = transaction
        .fetch_one(add_field)
        .await
        .map_err(QueryError::ExecuteFailed)?;

    let field_id: i32 = row.get("field_id");
    let full_name = ty.name.clone() + "." + &field.name;
    let add_field_name = add_field_name.bind(full_name).bind(field_id);
    transaction
        .execute(add_field_name)
        .await
        .map_err(QueryError::ExecuteFailed)?;

    for label in &field.labels {
        let q = sqlx::query("INSERT INTO field_labels (label_name, field_id) VALUES ($1, $2)");
        transaction
            .execute(q.bind(label).bind(field_id))
            .await
            .map_err(QueryError::ExecuteFailed)?;
    }
    Ok(())
}

impl MetaService {
    pub(crate) fn new(kind: Kind, pool: AnyPool) -> Self {
        Self { kind, pool }
    }

    pub(crate) async fn local_connection(conn: &DbConnection) -> anyhow::Result<Self> {
        let local = conn.local_connection().await?;
        Ok(Self::new(local.kind, local.pool))
    }

    /// Create the schema of the underlying metadata store.
    pub(crate) async fn create_schema(&self) -> anyhow::Result<()> {
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

    /// Load the existing endpoints from from metadata store.
    pub(crate) async fn load_endpoints<'r>(&self) -> anyhow::Result<RoutePaths> {
        let query = sqlx::query("SELECT path, code FROM endpoints");
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(QueryError::FetchFailed)?;

        let mut routes = RoutePaths::default();
        for row in rows {
            let path: &str = row.get("path");
            let code: &str = row.get("code");
            debug!("Loading endpoint {}", path);

            let func = Box::new({
                let path = path.to_string();
                move |req| deno::run_js(path.clone(), req).boxed_local()
            });
            routes.add_route(path, code, func);
        }
        Ok(routes)
    }

    pub(crate) async fn persist_endpoints(&self, routes: &RoutePaths) -> anyhow::Result<()> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;

        let drop = sqlx::query("DELETE from endpoints");
        transaction
            .execute(drop)
            .await
            .map_err(QueryError::ExecuteFailed)?;

        for (path, code) in routes.route_data() {
            let new_route = sqlx::query("INSERT INTO endpoints (path, code) VALUES ($1, $2)")
                .bind(path.to_str())
                .bind(code);

            transaction
                .execute(new_route)
                .await
                .map_err(QueryError::ExecuteFailed)?;
        }
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }

    /// Load the type system from metadata store.
    pub(crate) async fn load_type_system<'r>(&self) -> anyhow::Result<TypeSystem> {
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
            ts.add_type(Arc::new(ObjectType {
                id: Some(type_id),
                name: type_name.to_string(),
                fields,
                backing_table: backing_table.to_string(),
            }))?;
        }
        Ok(ts)
    }

    async fn load_type_fields(&self, ts: &TypeSystem, type_id: i32) -> anyhow::Result<Vec<Field>> {
        let query = sqlx::query("SELECT fields.field_id AS field_id, field_names.field_name AS field_name, fields.field_type AS field_type, fields.default_value as default_value, fields.is_optional as is_optional FROM field_names INNER JOIN fields WHERE fields.type_id = $1 AND field_names.field_id = fields.field_id;");
        let query = query.bind(type_id);
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(QueryError::FetchFailed)?;
        let mut fields = Vec::new();
        for row in rows {
            let field_name: &str = row.get("field_name");
            let field_name = field_name.split_once('.').unwrap().1;
            let field_type: &str = row.get("field_type");
            let field_id: i32 = row.get("field_id");
            let field_def: Option<String> = row.get("default_value");
            let is_optional: bool = row.get("is_optional");
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
                id: Some(field_id),
                name: field_name.to_string(),
                type_: ty,
                labels,
                default: field_def,
                is_optional,
            });
        }
        Ok(fields)
    }

    pub(crate) async fn remove_type(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> anyhow::Result<()> {
        let type_id = ty
            .id
            .ok_or_else(|| anyhow!("logical error. Trying to delete type without id"))?;

        for field in &ty.fields {
            remove_field_query(transaction, field).await?;
        }

        let del_type = sqlx::query("DELETE FROM types WHERE type_id = $1");
        let del_type_name = sqlx::query("DELETE FROM type_names WHERE type_id = $1");

        transaction
            .execute(del_type.bind(type_id))
            .await
            .map_err(QueryError::ExecuteFailed)?;

        transaction
            .execute(del_type_name.bind(type_id))
            .await
            .map_err(QueryError::ExecuteFailed)?;

        Ok(())
    }

    pub(crate) async fn update_type(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
        delta: ObjectDelta,
    ) -> anyhow::Result<()> {
        for field in delta.added_fields.iter() {
            insert_field_query(transaction, ty, None, field).await?;
        }

        for field in delta.removed_fields.iter() {
            remove_field_query(transaction, field).await?;
        }

        for field in delta.updated_fields.iter() {
            update_field_query(transaction, field).await?;
        }
        Ok(())
    }

    pub(crate) async fn start_transaction(&self) -> anyhow::Result<Transaction<'_, Any>> {
        Ok(self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?)
    }

    pub(crate) async fn commit_transaction(
        transaction: Transaction<'_, Any>,
    ) -> anyhow::Result<()> {
        transaction
            .commit()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        Ok(())
    }

    pub(crate) async fn insert_type(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> anyhow::Result<()> {
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
            insert_field_query(transaction, ty, Some(id), field).await?;
        }
        Ok(())
    }

    pub(crate) async fn new_session_token(&self, username: &str) -> Result<String, QueryError> {
        let token = Uuid::new_v4().to_string();
        // TODO: Expire tokens.
        let insert = sqlx::query("INSERT INTO sessions(token, username) VALUES($1, $2)")
            .bind(&token)
            .bind(username);
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        transaction
            .execute(insert)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(token)
    }

    pub(crate) async fn get_username(&self, token: &str) -> Result<String, QueryError> {
        let row = sqlx::query("SELECT username FROM sessions WHERE token=$1")
            .bind(token)
            .fetch_all(&self.pool)
            .await
            .map_err(QueryError::FetchFailed)?
            .pop()
            .ok_or_else(|| QueryError::TokenNotFound(token.into()))?;
        let username: &str = row.get("username");
        Ok(username.into())
    }
}
