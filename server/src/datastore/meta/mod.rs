pub(crate) mod schema;

// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiInfo, ApiInfoMap};
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

async fn fetch_all<'a, E>(
    executor: E,
    query: sqlx::query::Query<'a, sqlx::Any, sqlx::any::AnyArguments<'a>>,
) -> anyhow::Result<Vec<sqlx::any::AnyRow>>
where
    E: Executor<'a, Database = sqlx::Any>,
{
    let qstr = query.sql();
    query
        .fetch_all(executor)
        .await
        .with_context(|| format!("Executing query {}", qstr))
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
            ", default_value = $5"
        };

        let querystr = format!(
            r#"
            UPDATE fields
            SET
                field_type = $1,
                is_optional = $2::bool,
                is_unique = $4::bool {default_stmt}
            WHERE field_id = $4"#
        );
        let mut query = sqlx::query(&querystr);

        query = query
            .bind(field.type_.name())
            .bind(field.is_optional)
            .bind(field.is_unique)
            .bind(field_id);

        if let Some(value) = &field.default {
            query = query.bind(value.to_owned());
        }

        execute!(transaction, query)?;
    }

    if let Some(labels) = &delta.labels {
        let flush = sqlx::query("DELETE FROM field_labels WHERE field_id = $1").bind(field_id);
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
        .context("logical error. Trying to delete field without id")?;

    let query = sqlx::query("DELETE FROM fields WHERE field_id = $1").bind(field_id);
    execute!(transaction, query)?;

    let query = sqlx::query("DELETE FROM field_names WHERE field_id = $1").bind(field_id);
    execute!(transaction, query)?;

    let query = sqlx::query("DELETE FROM field_labels WHERE field_id = $1").bind(field_id);
    execute!(transaction, query)?;

    Ok(())
}

async fn insert_field_query(
    transaction: &mut Transaction<'_, Any>,
    ty: &ObjectType,
    recently_added_type_id: Option<i32>,
    field: &Field,
) -> anyhow::Result<()> {
    let type_id = ty.meta_id.xor(recently_added_type_id).context(
        "logical error. Seems like a type is at the same type pre-existing and recently added??",
    )?;

    let add_field = match &field.user_provided_default() {
        None => {
            let query = sqlx::query(
                r#"
                INSERT INTO fields (field_type, type_id, is_optional, is_unique)
                VALUES ($1, $2, $3, $4)
                RETURNING *"#,
            );
            query
                .bind(field.type_.name())
                .bind(type_id)
                .bind(field.is_optional)
                .bind(field.is_unique)
        }
        Some(value) => {
            let query = sqlx::query(
                r#"
                INSERT INTO fields (
                    field_type,
                    type_id,
                    default_value,
                    is_optional,
                    is_unique)
                VALUES ($1, $2, $3, $4, $5)
                RETURNING *"#,
            );
            query
                .bind(field.type_.name())
                .bind(type_id)
                .bind(value.to_owned())
                .bind(field.is_optional)
                .bind(field.is_unique)
        }
    };
    let add_field_name = sqlx::query(
        r#"
        INSERT INTO field_names (field_name, field_id)
        VALUES ($1, $2)"#,
    );

    let row = fetch_one!(transaction, add_field)?;

    let field_id: i32 = row.get("field_id");
    let full_name = field.persisted_name(ty);

    let split = full_name.split('.').count();
    anyhow::ensure!(split == 3, "Expected version and type information as part of the field name. Got {}. Should have caught sooner! Aborting", full_name);

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

    async fn get_version(transaction: &mut Transaction<'_, Any>) -> anyhow::Result<String> {
        let query = sqlx::query(
            "SELECT version FROM chisel_version WHERE version_id = 'chiselstrike' LIMIT 1",
        );
        match fetch_all(transaction, query).await {
            Err(_) => Ok("0".to_string()),
            Ok(rows) => {
                if let Some(row) = rows.into_iter().next() {
                    let v: &str = row.get("version");
                    Ok(v.to_string())
                } else {
                    Ok("0".into())
                }
            }
        }
    }

    async fn count_tables(&self, transaction: &mut Transaction<'_, Any>) -> anyhow::Result<usize> {
        let query = match self.kind {
            Kind::Sqlite => sqlx::query(
                r#"
                SELECT name
                FROM sqlite_schema
                WHERE type ='table' AND name NOT LIKE 'sqlite_%'"#,
            ),
            Kind::Postgres => sqlx::query(
                r#"
                SELECT tablename AS name
                FROM pg_catalog.pg_tables
                WHERE schemaname != 'pg_catalog' AND schemaname != 'information_schema'"#,
            ),
        };
        let rows = fetch_all(transaction, query).await?;
        Ok(rows.len())
    }

    /// Create the schema of the underlying metadata store.
    pub(crate) async fn create_schema(&self) -> anyhow::Result<()> {
        let query_builder = DbConnection::get_query_builder(&self.kind);
        let tables = schema::tables();

        let mut transaction = self.start_transaction().await?;
        // The chisel_version table is relatively new, so if it doesn't exist
        // it could be that this is either a new installation, or an upgrade. So
        // we query something that was with us from the beginning to tell those apart
        let new_install = self.count_tables(&mut transaction).await? == 0;

        for table in tables {
            let query = table.build_any(query_builder);
            let query = sqlx::query(&query);
            execute!(transaction, query)?;
        }

        if new_install {
            let query =
                sqlx::query("INSERT INTO chisel_version (version, version_id) VALUES ($1, $2)")
                    .bind(schema::CURRENT_VERSION)
                    .bind("chiselstrike");
            execute!(transaction, query)?;
        }

        let mut version = Self::get_version(&mut transaction).await?;

        while version != schema::CURRENT_VERSION {
            let (statements, new_version) = schema::evolve_from(&version).await?;

            for statement in statements {
                let query = statement.build_any(query_builder);
                let query = sqlx::query(&query);
                execute!(transaction, query)?;
            }

            let query = sqlx::query(
                r#"
                INSERT INTO chisel_version (version, version_id)
                VALUES ($1, $2)
                ON CONFLICT(version_id) DO UPDATE SET version = $1
                WHERE chisel_version.version_id = $2"#,
            )
            .bind(new_version.as_str())
            .bind("chiselstrike");

            execute!(transaction, query)?;
            version = new_version;
        }

        Self::commit_transaction(transaction).await?;
        Ok(())
    }

    /// Load information about the current API versions present in this system
    pub(crate) async fn load_api_info(&self) -> anyhow::Result<ApiInfoMap> {
        let query = sqlx::query("SELECT api_version, app_name, version_tag FROM api_info");
        let rows = fetch_all(&self.pool, query).await?;

        let mut info = ApiInfoMap::default();
        for row in rows {
            let api_version: &str = row.get("api_version");
            let app_name: &str = row.get("app_name");
            let tag: &str = row.get("version_tag");

            debug!("Loading api version info for {}", api_version);
            info.insert(api_version.into(), ApiInfo::new(app_name, tag));
        }
        Ok(info)
    }

    pub(crate) async fn persist_api_info(
        &self,
        transaction: &mut Transaction<'_, Any>,
        api_version: &str,
        info: &ApiInfo,
    ) -> anyhow::Result<()> {
        let add_api = sqlx::query(
            r#"
            INSERT INTO api_info (api_version, app_name, version_tag)
            VALUES ($1, $2, $3)
            ON CONFLICT(api_version) DO UPDATE SET app_name = $2, version_tag = $3
            WHERE api_info.api_version = $1"#,
        )
        .bind(api_version.to_owned())
        .bind(info.name.clone())
        .bind(info.tag.clone());
        execute!(transaction, add_api)?;
        Ok(())
    }

    /// Load the existing endpoints from from metadata store.
    pub(crate) async fn load_endpoints<'r>(&self) -> anyhow::Result<PrefixMap<String>> {
        let query = sqlx::query("SELECT path, code FROM endpoints");
        let rows = fetch_all(&self.pool, query).await?;

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

        let drop = sqlx::query("DELETE FROM endpoints");
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
        let query = sqlx::query(
            r#"
            SELECT
                types.type_id AS type_id,
                types.backing_table AS backing_table,
                type_names.name AS type_name
            FROM types
            INNER JOIN type_names ON types.type_id = type_names.type_id"#,
        );
        let rows = fetch_all(&self.pool, query).await?;

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
        let query = sqlx::query(
            r#"
            SELECT
                fields.field_id AS field_id,
                field_names.field_name AS field_name,
                fields.field_type AS field_type,
                fields.default_value AS default_value,
                fields.is_optional AS is_optional,
                fields.is_unique AS is_unique
            FROM field_names
            INNER JOIN fields
                ON fields.type_id = $1 AND field_names.field_id = fields.field_id;"#,
        );
        let query = query.bind(type_id);
        let rows = fetch_all(&self.pool, query).await?;

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
            let is_unique: bool = row.get("is_unique");

            let labels_query =
                sqlx::query("SELECT label_name FROM field_labels WHERE field_id = $1");

            let query = labels_query.bind(field_id);

            let rows = fetch_all(&self.pool, query).await?;

            let labels = rows
                .iter()
                .map(|r| r.get("label_name"))
                .collect::<Vec<String>>();

            fields.push(Field::new(desc, labels, field_def, is_optional, is_unique));
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
            .context("logical error. Trying to delete type without id")?;

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
        let add_policy = sqlx::query(
            r#"
            INSERT INTO policies (policy_str, version)
            VALUES ($1, $2)
            ON CONFLICT(version) DO UPDATE SET policy_str = $1
            WHERE policies.version = $2"#,
        )
        .bind(policy.to_owned())
        .bind(version.to_owned());
        execute!(transaction, add_policy)?;
        Ok(())
    }

    pub(crate) async fn delete_policy_version(
        &self,
        transaction: &mut Transaction<'_, Any>,
        version: &str,
    ) -> anyhow::Result<()> {
        let delete_policy =
            sqlx::query("DELETE FROM policies WHERE version = $1").bind(version.to_owned());
        execute!(transaction, delete_policy)?;
        Ok(())
    }

    /// Loads all policies, for all versions.
    ///
    /// Useful on startup, when we have to populate our in-memory state from the meta database.
    pub(crate) async fn load_policies(&self) -> anyhow::Result<Policies> {
        let get_policy = sqlx::query("SELECT version, policy_str FROM policies");

        let rows = fetch_all(&self.pool, get_policy).await?;

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
        let insert = sqlx::query("INSERT INTO sessions(token, user_id) VALUES($1::uuid, $2)")
            .bind(&token)
            .bind(userid);
        let mut transaction = self.pool.begin().await?;

        execute!(transaction, insert)?;

        transaction.commit().await?;
        Ok(token)
    }

    pub(crate) async fn get_user_id(&self, token: &str) -> anyhow::Result<String> {
        let query = sqlx::query("SELECT user_id FROM sessions WHERE token=$1::uuid").bind(token);

        let mut rows = fetch_all(&self.pool, query).await?;
        let row = rows
            .pop()
            .ok_or_else(|| anyhow!("token {} not found", token))?;
        let id: &str = row.get("user_id");
        Ok(id.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tempdir::TempDir;

    // test that we can open and successfully evolve 0.6 to the current version
    #[tokio::test]
    async fn evolve_0_6() -> Result<()> {
        let tmp_dir = TempDir::new("evolve_0_6")?;
        let file_path = tmp_dir.path().join("chisel.db");
        tokio::fs::copy("./test_files/chiseld-0.6.db", &file_path)
            .await
            .unwrap();

        let conn_str = format!("sqlite://{}?mode=rwc", file_path.display());

        let meta_conn = DbConnection::connect(&conn_str, 1).await?;
        let meta = MetaService::local_connection(&meta_conn, 1).await.unwrap();

        let mut transaction = meta.start_transaction().await.unwrap();
        let version = MetaService::get_version(&mut transaction).await.unwrap();
        MetaService::commit_transaction(transaction).await.unwrap();
        assert_eq!(version, "0");

        meta.create_schema().await.unwrap();
        let mut transaction = meta.start_transaction().await.unwrap();
        let version = MetaService::get_version(&mut transaction).await.unwrap();
        MetaService::commit_transaction(transaction).await.unwrap();
        assert_eq!(version, schema::CURRENT_VERSION);

        // evolving again works (idempotency, we don't fail)
        meta.create_schema().await.unwrap();
        let mut transaction = meta.start_transaction().await.unwrap();
        let version = MetaService::get_version(&mut transaction).await.unwrap();
        MetaService::commit_transaction(transaction).await.unwrap();
        assert_eq!(version, schema::CURRENT_VERSION);

        Ok(())
    }
}
