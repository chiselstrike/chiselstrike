pub(crate) mod schema;

// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::{DbConnection, Kind};
use crate::policies::Policies;
use crate::prefix_map::PrefixMap;
use crate::types::{
    ExistingField, ExistingObject, Field, FieldDelta, ObjectDelta, ObjectType, TypeSystem,
};
use anyhow::{anyhow, Context};
use sqlx::any::{Any, AnyPool};
use sqlx::{Execute, Executor, Row, Transaction};
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

macro_rules! execute {
    ( $transaction:expr, $query:expr ) => {{
        let qstr = $query.sql();
        $transaction
            .execute($query)
            .await
            .with_context(|| format!("Executing query {}", qstr))
    }};
}

macro_rules! fetch_one {
    ( $transaction:expr, $query:expr) => {{
        let qstr = $query.sql();
        $transaction
            .fetch_one($query)
            .await
            .with_context(|| format!("Executing query {}", qstr))
    }};
}

macro_rules! fetch_all {
    ( $pool:expr, $query:expr) => {{
        let qstr = $query.sql();
        $query
            .fetch_all($pool)
            .await
            .with_context(|| format!("Executing query {}", qstr))
    }};
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

        execute!(transaction, query)?;
    }

    if let Some(labels) = &delta.labels {
        let flush = sqlx::query("DELETE FROM field_labels where field_id = $1").bind(field_id);
        execute!(transaction, flush)?;

        for label in labels.iter() {
            let q = sqlx::query("INSERT INTO field_labels (label_name, field_id) VALUES ($1, $2)")
                .bind(label)
                .bind(field_id);
            execute!(transaction, q)?;
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

    let query = sqlx::query("DELETE FROM fields WHERE field_id = $1").bind(field_id);
    execute!(transaction, query)?;

    let query = sqlx::query("DELETE from field_names WHERE field_id = $1").bind(field_id);
    execute!(transaction, query)?;

    let query = sqlx::query("DELETE from field_labels WHERE field_id = $1").bind(field_id);
    execute!(transaction, query)?;

    Ok(())
}

async fn insert_field_query(
    transaction: &mut Transaction<'_, Any>,
    ty: &ObjectType,
    recently_added_type_id: Option<i32>,
    field: &Field,
) -> anyhow::Result<()> {
    let type_id = ty.meta_id.xor(recently_added_type_id).ok_or_else(|| anyhow!("logical error. Seems like a type is at the same type pre-existing and recently added??"))?;

    let add_field = match &field.user_provided_default() {
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

    let row = fetch_one!(transaction, add_field)?;

    let field_id: i32 = row.get("field_id");
    let full_name = field.persisted_name(ty);
    let add_field_name = add_field_name.bind(full_name).bind(field_id);
    execute!(transaction, add_field_name)?;

    for label in &field.labels {
        let q = sqlx::query("INSERT INTO field_labels (label_name, field_id) VALUES ($1, $2)")
            .bind(label)
            .bind(field_id);
        execute!(transaction, q)?;
    }
    Ok(())
}

impl MetaService {
    pub(crate) fn new(kind: Kind, pool: AnyPool) -> Self {
        Self { kind, pool }
    }

    pub(crate) async fn local_connection(
        conn: &DbConnection,
        nr_conn: usize,
    ) -> anyhow::Result<Self> {
        let local = conn.local_connection(nr_conn).await?;
        Ok(Self::new(local.kind, local.pool))
    }

    /// Create the schema of the underlying metadata store.
    pub(crate) async fn create_schema(&self) -> anyhow::Result<()> {
        let query_builder = DbConnection::get_query_builder(&self.kind);
        let tables = schema::tables();
        let mut conn = self.pool.acquire().await?;
        for table in tables {
            let query = table.build_any(query_builder);
            let query = sqlx::query(&query);
            conn.execute(query).await?;
        }
        Ok(())
    }

    /// Load the existing endpoints from from metadata store.
    pub(crate) async fn load_endpoints<'r>(&self) -> anyhow::Result<PrefixMap<String>> {
        let query = sqlx::query("SELECT path, code FROM endpoints");
        let rows = fetch_all!(&self.pool, query)?;

        let mut routes = PrefixMap::default();
        for row in rows {
            let path: &str = row.get("path");
            let code: &str = row.get("code");
            debug!("Loading endpoint {}", path);
            routes.insert(path.into(), code.to_string());
        }
        Ok(routes)
    }

    pub(crate) async fn persist_endpoints(&self, routes: &PrefixMap<String>) -> anyhow::Result<()> {
        let mut transaction = self.pool.begin().await?;

        let drop = sqlx::query("DELETE from endpoints");
        execute!(transaction, drop)?;

        for (path, code) in routes.iter() {
            let new_route = sqlx::query("INSERT INTO endpoints (path, code) VALUES ($1, $2)")
                .bind(path.to_str())
                .bind(code);

            execute!(transaction, new_route)?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Load the type system from metadata store.
    pub(crate) async fn load_type_system<'r>(&self) -> anyhow::Result<TypeSystem> {
        let query = sqlx::query("SELECT types.type_id AS type_id, types.backing_table AS backing_table, type_names.name AS type_name FROM types INNER JOIN type_names ON types.type_id = type_names.type_id");
        let rows = fetch_all!(&self.pool, query)?;

        let mut ts = TypeSystem::default();
        for row in rows {
            let type_id: i32 = row.get("type_id");
            let backing_table: &str = row.get("backing_table");
            let type_name: &str = row.get("type_name");
            let desc = ExistingObject::new(type_name, backing_table, type_id)?;
            let fields = self.load_type_fields(&ts, type_id).await?;

            let ty = ObjectType::new(desc, fields)?;
            ts.add_type(Arc::new(ty))?;
        }
        Ok(ts)
    }

    async fn load_type_fields(&self, ts: &TypeSystem, type_id: i32) -> anyhow::Result<Vec<Field>> {
        let query = sqlx::query("SELECT fields.field_id AS field_id, field_names.field_name AS field_name, fields.field_type AS field_type, fields.default_value as default_value, fields.is_optional as is_optional FROM field_names INNER JOIN fields ON fields.type_id = $1 AND field_names.field_id = fields.field_id;");
        let query = query.bind(type_id);
        let rows = fetch_all!(&self.pool, query)?;

        let mut fields = Vec::new();
        for row in rows {
            let db_field_name: &str = row.get("field_name");
            let field_id: i32 = row.get("field_id");
            let field_type: &str = row.get("field_type");

            let split: Vec<&str> = db_field_name.split('.').collect();
            anyhow::ensure!(split.len() == 3, "Expected version and type information as part of the field name. Got {}. Database corrupted?", db_field_name);
            let field_name = split[2].to_owned();
            let version = split[0].to_owned();
            let desc = ExistingField::new(
                &field_name,
                ts.lookup_type(field_type, &version)?,
                field_id,
                &version,
            );

            let field_def: Option<String> = row.get("default_value");
            let is_optional: bool = row.get("is_optional");

            let labels_query =
                sqlx::query("SELECT label_name FROM field_labels WHERE field_id = $1");

            let query = labels_query.bind(field_id);

            let rows = fetch_all!(&self.pool, query)?;

            let labels = rows
                .iter()
                .map(|r| r.get("label_name"))
                .collect::<Vec<String>>();

            fields.push(Field::new(desc, labels, field_def, is_optional));
        }
        Ok(fields)
    }

    pub(crate) async fn remove_type(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> anyhow::Result<()> {
        let type_id = ty
            .meta_id
            .ok_or_else(|| anyhow!("logical error. Trying to delete type without id"))?;

        for field in ty.user_fields() {
            remove_field_query(transaction, field).await?;
        }

        let del_type = sqlx::query("DELETE FROM types WHERE type_id = $1").bind(type_id);
        let del_type_name = sqlx::query("DELETE FROM type_names WHERE type_id = $1").bind(type_id);

        execute!(transaction, del_type)?;
        execute!(transaction, del_type_name)?;

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
        Ok(self.pool.begin().await?)
    }

    pub(crate) async fn commit_transaction(
        transaction: Transaction<'_, Any>,
    ) -> anyhow::Result<()> {
        transaction.commit().await?;
        Ok(())
    }

    /// Persist a specific policy version.
    ///
    /// We don't have a method that persist all policies, for all versions, because
    /// versions are applied independently
    pub(crate) async fn persist_policy_version(
        &self,
        transaction: &mut Transaction<'_, Any>,
        version: &str,
        policy: &str,
    ) -> anyhow::Result<()> {
        let add_policy = sqlx::query("INSERT INTO policies (policy_str, version) VALUES ($1, $2) ON CONFLICT(version) DO UPDATE SET policy_str = $1 WHERE policies.version = $2").bind(policy.to_owned()).bind(version.to_owned());
        execute!(transaction, add_policy)?;
        Ok(())
    }

    pub(crate) async fn delete_policy_version(
        &self,
        transaction: &mut Transaction<'_, Any>,
        version: &str,
    ) -> anyhow::Result<()> {
        let delete_policy =
            sqlx::query("DELETE from policies WHERE version = $1").bind(version.to_owned());
        execute!(transaction, delete_policy)?;
        Ok(())
    }

    /// Loads all policies, for all versions.
    ///
    /// Useful on startup, when we have to populate our in-memory state from the meta database.
    pub(crate) async fn load_policies(&self) -> anyhow::Result<Policies> {
        let get_policy = sqlx::query("SELECT version, policy_str FROM policies");

        let rows = fetch_all!(&self.pool, get_policy)?;

        let mut policies = Policies::default();
        for row in rows {
            let version: &str = row.get("version");
            let yaml: &str = row.get("policy_str");

            policies.add_from_yaml(version, yaml)?;
        }
        Ok(policies)
    }

    pub(crate) async fn insert_type(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> anyhow::Result<()> {
        let add_type = sqlx::query("INSERT INTO types (backing_table) VALUES ($1) RETURNING *");
        let add_type_name = sqlx::query("INSERT INTO type_names (type_id, name) VALUES ($1, $2)");

        let add_type = add_type.bind(ty.backing_table().to_owned());
        let row = fetch_one!(transaction, add_type)?;

        let id: i32 = row.get("type_id");
        let add_type_name = add_type_name.bind(id).bind(ty.persisted_name());
        execute!(transaction, add_type_name)?;

        for field in ty.user_fields() {
            insert_field_query(transaction, ty, Some(id), field).await?;
        }
        Ok(())
    }

    pub(crate) async fn new_session_token(&self, userid: &str) -> anyhow::Result<String> {
        let token = Uuid::new_v4().to_string();
        // TODO: Expire tokens.
        let insert = sqlx::query("INSERT INTO sessions(token, user_id) VALUES($1, $2)")
            .bind(&token)
            .bind(userid);
        let mut transaction = self.pool.begin().await?;

        execute!(transaction, insert)?;

        transaction.commit().await?;
        Ok(token)
    }

    pub(crate) async fn get_user_id(&self, token: &str) -> anyhow::Result<String> {
        let query = sqlx::query("SELECT user_id FROM sessions WHERE token=$1").bind(token);

        let mut rows = fetch_all!(&self.pool, query)?;
        let row = rows
            .pop()
            .ok_or_else(|| anyhow!("token {} not found", token))?;
        let id: &str = row.get("user_id");
        Ok(id.into())
    }
}
