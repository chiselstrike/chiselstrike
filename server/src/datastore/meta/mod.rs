// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

mod migrate;
mod migrate_to_2;
mod schema;

use crate::datastore::DbConnection;
use crate::policies::PolicySystem;
use crate::types::{
    BuiltinTypes, DbIndex, Entity, ExistingField, ExistingObject, Field, FieldDelta, ObjectDelta,
    ObjectDescriptor, ObjectType, TypeSystem,
};
use crate::version::VersionInfo;
use anyhow::{Context, Result};
use sqlx::any::{Any, AnyKind};
use sqlx::{Execute, Executor, Row, Transaction};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;

/// Meta service.
///
/// The meta service is responsible for managing metadata such as object
/// types and labels persistently.
#[derive(Debug, Clone)]
pub struct MetaService {
    db: Arc<DbConnection>,
}

async fn execute<'a, 'b>(
    transaction: &mut Transaction<'b, sqlx::Any>,
    query: sqlx::query::Query<'a, sqlx::Any, sqlx::any::AnyArguments<'a>>,
) -> Result<sqlx::any::AnyQueryResult> {
    let qstr = query.sql();
    transaction
        .execute(query)
        .await
        .with_context(|| format!("Failed to execute query {}", qstr))
}

async fn fetch_one<'a, 'b>(
    transaction: &mut Transaction<'b, sqlx::Any>,
    query: sqlx::query::Query<'a, sqlx::Any, sqlx::any::AnyArguments<'a>>,
) -> Result<sqlx::any::AnyRow> {
    let qstr = query.sql();
    transaction
        .fetch_one(query)
        .await
        .with_context(|| format!("Failed to execute query {}", qstr))
}

async fn fetch_all<'a, E>(
    executor: E,
    query: sqlx::query::Query<'a, sqlx::Any, sqlx::any::AnyArguments<'a>>,
) -> Result<Vec<sqlx::any::AnyRow>>
where
    E: Executor<'a, Database = sqlx::Any>,
{
    let qstr = query.sql();
    query
        .fetch_all(executor)
        .await
        .with_context(|| format!("Failed to execute query {}", qstr))
}

async fn file_exists(file: &Path) -> Result<bool> {
    match fs::metadata(file).await {
        Ok(_) => Ok(true),
        Err(x) => match x.kind() {
            std::io::ErrorKind::NotFound => Ok(false),
            _ => {
                anyhow::bail!("Can't read {}", file.display());
            }
        },
    }
}

async fn update_field_query(
    transaction: &mut Transaction<'_, Any>,
    delta: &FieldDelta,
) -> Result<()> {
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
                is_unique = $3::bool {default_stmt}
            WHERE field_id = $4"#
        );
        let mut query = sqlx::query(&querystr);

        query = query
            .bind(field.type_id.name())
            .bind(field.is_optional)
            .bind(field.is_unique)
            .bind(field_id);

        if let Some(value) = &field.default {
            query = query.bind(value.to_owned());
        }

        execute(transaction, query).await?;
    }

    if let Some(labels) = &delta.labels {
        let flush = sqlx::query("DELETE FROM field_labels WHERE field_id = $1").bind(field_id);
        execute(transaction, flush).await?;

        for label in labels.iter() {
            let q = sqlx::query("INSERT INTO field_labels (label_name, field_id) VALUES ($1, $2)")
                .bind(label)
                .bind(field_id);
            execute(transaction, q).await?;
        }
    }
    Ok(())
}

async fn remove_field_query(transaction: &mut Transaction<'_, Any>, field: &Field) -> Result<()> {
    let field_id = field
        .id
        .context("logical error. Trying to delete field without id")?;

    let query = sqlx::query("DELETE FROM fields WHERE field_id = $1").bind(field_id);
    execute(transaction, query).await?;

    let query = sqlx::query("DELETE FROM field_names WHERE field_id = $1").bind(field_id);
    execute(transaction, query).await?;

    let query = sqlx::query("DELETE FROM field_labels WHERE field_id = $1").bind(field_id);
    execute(transaction, query).await?;

    Ok(())
}

async fn insert_field_query(
    transaction: &mut Transaction<'_, Any>,
    ty: &ObjectType,
    recently_added_type_id: Option<i32>,
    field: &Field,
) -> Result<()> {
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
                .bind(field.type_id.name())
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
                .bind(field.type_id.name())
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

    let row = fetch_one(transaction, add_field).await?;

    let field_id: i32 = row.get("field_id");
    let full_name = field.persisted_name(ty);

    let split = full_name.split('.').count();
    anyhow::ensure!(split == 3, "Expected version and type information as part of the field name. Got {}. Should have caught sooner! Aborting", full_name);

    let add_field_name = add_field_name.bind(full_name).bind(field_id);
    execute(transaction, add_field_name).await?;

    for label in &field.labels {
        let q = sqlx::query("INSERT INTO field_labels (label_name, field_id) VALUES ($1, $2)")
            .bind(label)
            .bind(field_id);
        execute(transaction, q).await?;
    }
    Ok(())
}

impl MetaService {
    pub fn new(db: Arc<DbConnection>) -> Self {
        Self { db }
    }

    async fn get_schema_version(&self, transaction: &mut Transaction<'_, Any>) -> Result<String> {
        let tables = self.list_tables(transaction).await?;

        if tables.contains("chisel_version") {
            let query = sqlx::query(
                "SELECT version FROM chisel_version WHERE version_id = 'chiselstrike' LIMIT 1",
            );

            let rows = fetch_all(&mut *transaction, query).await?;
            if let Some(row) = rows.into_iter().next() {
                let v: &str = row.get("version");
                return Ok(v.to_string());
            }
        }

        Ok(if tables.is_empty() {
            // the database is completely empty
            "empty".into()
        } else {
            // the database comes from a server that did not use schema versioning
            "0".into()
        })
    }

    /// Try to migrate an old-style (split meta + data) sqlite layout to
    /// a single file.
    ///
    /// For Postgres this is a lot more complex because it is not possible to
    /// do cross-database transactions. But this handles the local dev case,
    /// which is what is most relevant at the moment.
    pub async fn maybe_migrate_split_sqlite_database(
        &self,
        sources: &[PathBuf],
        to: &str,
    ) -> Result<()> {
        match self.db.pool.any_kind() {
            AnyKind::Sqlite => {}
            _ => anyhow::bail!("Can't migrate postgres tables yet"),
        }

        // For the old files, either none of them exists (in which case
        // we're ok), or all of them exists. The case in which some of them
        // exists we're better off not touching, and erroring out.
        let mut not_found = vec![];
        for src in sources {
            if !file_exists(src).await? {
                not_found.push(src);
            }
        }

        if not_found.len() == sources.len() {
            return Ok(());
        }

        if !not_found.is_empty() {
            anyhow::bail!(
                "Some of the old sqlite files were not found: {:?}",
                not_found
            );
        }

        let mut transaction = self.begin_transaction().await?;
        match self
            .maybe_migrate_split_sqlite_database_inner(&mut transaction, sources)
            .await
        {
            Err(x) => Err(x),
            Ok(true) => {
                let mut full_str = format!(
                    "Migrated your data to {}. You can now delete the following files:",
                    to,
                );
                for src in sources {
                    let s = src.display();
                    write!(full_str, "\n\t{}\n\t{}-wal\n\t{}-shm", s, s, s).unwrap();
                }

                info!("{}", &full_str);
                Ok(())
            }
            Ok(false) => Ok(()),
        }?;
        Self::commit_transaction(transaction).await?;
        Ok(())
    }

    async fn maybe_migrate_split_sqlite_database_inner(
        &self,
        transaction: &mut Transaction<'_, Any>,
        sources: &[PathBuf],
    ) -> Result<bool> {
        // this sqlite instance already has data, nothing to migrate.
        if !self.list_tables(transaction).await?.is_empty() {
            return Ok(false);
        }

        for (idx, src) in sources.iter().enumerate() {
            let db = format!("db{}", idx);

            let attach = format!("attach database '{}' as '{}'", src.display(), db);
            execute(transaction, sqlx::query::<sqlx::Any>(&attach)).await?;

            let query = format!(
                r#"
                    SELECT sql,name
                    FROM '{}'.sqlite_schema
                    WHERE type ='table' AND name NOT LIKE 'sqlite_%'"#,
                db
            );
            // function takes an Executor, which is causing the compiler to move this.
            // so &mut * it
            let rows = fetch_all(&mut *transaction, sqlx::query::<sqlx::Any>(&query)).await?;
            for row in rows {
                let sql: &str = row.get("sql");
                let sql = sql.replace("CREATE TABLE", "CREATE TABLE IF NOT EXISTS");
                execute(transaction, sqlx::query::<sqlx::Any>(&sql)).await?;
                let name: &str = row.get("name");
                let copy = format!(
                    r#"
                    INSERT INTO '{}' SELECT * from '{}'.{}"#,
                    name, db, name
                );
                execute(transaction, sqlx::query::<sqlx::Any>(&copy)).await?;
            }
        }
        Ok(true)
    }

    async fn list_tables(&self, transaction: &mut Transaction<'_, Any>) -> Result<HashSet<String>> {
        let query = match self.db.pool.any_kind() {
            AnyKind::Sqlite => sqlx::query(
                r#"
                SELECT name
                FROM sqlite_schema
                WHERE type ='table' AND name NOT LIKE 'sqlite_%'"#,
            ),
            AnyKind::Postgres => sqlx::query(
                r#"
                SELECT tablename AS name
                FROM pg_catalog.pg_tables
                WHERE schemaname != 'pg_catalog' AND schemaname != 'information_schema'"#,
            ),
        };
        let rows = fetch_all(transaction, query).await?;
        Ok(rows.into_iter().map(|row| row.get("name")).collect())
    }

    /// Create the schema of the underlying metadata store.
    pub async fn migrate_schema(&self) -> Result<()> {
        let mut transaction = self.begin_transaction().await?;

        let mut version = self.get_schema_version(&mut transaction).await?;
        {
            let mut ctx = migrate::MigrateContext {
                query_builder: self.db.query_builder(),
                schema_builder: self.db.schema_builder(),
                transaction: &mut transaction,
            };
            // migrate the database to the latest version, step by step
            while let Some(new_version) = migrate::migrate_schema_step(&mut ctx, &version).await? {
                log::info!(
                    "Migrated database from version {:?} to version {:?}",
                    version,
                    new_version
                );
                version = new_version.into();
            }
        };

        // upsert the version in the database
        execute(
            &mut transaction,
            sqlx::query(
                r#"
                INSERT INTO chisel_version (version, version_id)
                VALUES ($1, $2)
                ON CONFLICT(version_id) DO UPDATE SET version = $1
                WHERE chisel_version.version_id = $2"#,
            )
            .bind(version.as_str())
            .bind("chiselstrike"),
        )
        .await?;

        Self::commit_transaction(transaction).await?;
        Ok(())
    }

    /// Load information about the current API versions present in this system
    pub async fn load_version_infos(&self) -> Result<HashMap<String, VersionInfo>> {
        let query = sqlx::query("SELECT api_version, app_name, version_tag FROM api_info");
        let rows = fetch_all(&self.db.pool, query).await?;

        let mut infos = HashMap::default();
        for row in rows {
            let version_id: String = row.get("api_version");
            let name: String = row.get("app_name");
            let tag: String = row.get("version_tag");

            debug!("Loading api version info for {}", version_id);
            infos.insert(version_id, VersionInfo { name, tag });
        }
        Ok(infos)
    }

    pub async fn persist_version_info(
        &self,
        transaction: &mut Transaction<'_, Any>,
        version_id: &str,
        info: &VersionInfo,
    ) -> Result<()> {
        let add_api = sqlx::query(
            r#"
            INSERT INTO api_info (api_version, app_name, version_tag)
            VALUES ($1, $2, $3)
            ON CONFLICT(api_version) DO UPDATE SET app_name = $2, version_tag = $3
            WHERE api_info.api_version = $1"#,
        )
        .bind(version_id.to_owned())
        .bind(info.name.clone())
        .bind(info.tag.clone());
        execute(transaction, add_api).await?;
        Ok(())
    }

    /// Load module source codes from metadata store.
    pub async fn load_modules(&self, version_id: &str) -> Result<HashMap<String, String>> {
        let query =
            sqlx::query("SELECT url, code FROM modules WHERE version = $1").bind(version_id);
        let rows = fetch_all(&self.db.pool, query).await?;
        let modules = rows
            .into_iter()
            .map(|row| {
                let url: String = row.get("url");
                let code: String = row.get("code");
                (url, code)
            })
            .collect();
        Ok(modules)
    }

    pub async fn persist_modules(
        &self,
        transaction: &mut Transaction<'_, Any>,
        version_id: &str,
        modules: &HashMap<String, String>,
    ) -> Result<()> {
        let drop = sqlx::query("DELETE FROM modules WHERE version = $1").bind(version_id);
        execute(transaction, drop).await?;

        for (url, code) in modules.iter() {
            let insert =
                sqlx::query("INSERT INTO modules (version, url, code) VALUES ($1, $2, $3)")
                    .bind(version_id)
                    .bind(url)
                    .bind(code);

            execute(transaction, insert).await?;
        }
        Ok(())
    }

    /// Load the type systems for all versions from metadata store.
    pub async fn load_type_systems(
        &self,
        builtin: &Arc<BuiltinTypes>,
    ) -> Result<HashMap<String, TypeSystem>> {
        let query = sqlx::query(
            r#"
            SELECT
                types.type_id AS type_id,
                types.backing_table AS backing_table,
                type_names.name AS type_name
            FROM types
            INNER JOIN type_names ON types.type_id = type_names.type_id"#,
        );
        let rows = fetch_all(&self.db.pool, query).await?;

        let mut type_systems = HashMap::new();
        let mut failures = vec![];

        for row in rows {
            let type_id: i32 = row.get("type_id");
            let backing_table: &str = row.get("backing_table");
            let type_name: &str = row.get("type_name");
            let desc = ExistingObject::new(type_name, backing_table, type_id)?;
            let ts = type_systems
                .entry(desc.version_id())
                .or_insert_with(|| TypeSystem::new(builtin.clone(), desc.version_id()));

            match self.load_type_fields(ts, type_id).await {
                Ok(fields) => {
                    let indexes = self.load_type_indexes(type_id, backing_table).await?;

                    let ty = ObjectType::new(&desc, fields, indexes)?;
                    ts.add_custom_type(Entity::Custom {
                        object: Arc::new(ty),
                        policy: None,
                    })?;
                }
                Err(_) => {
                    failures.push(row);
                }
            }
        }

        // Retry once for failures. The reason we may want to retry is that if you have a model (A)
        // that has another model (B) as a property, load_type_fields may fail, because B is not
        // yet loaded.
        //
        // In apply.rs, we have a topology sort to handle that so that models are created in the order
        // they are needed, but that only works if all models are inserted together. If you have a
        // pre-existing model (that already has an id), and then you add the new property, then
        // there isn't much we can do.
        for row in failures {
            let type_id: i32 = row.get("type_id");
            let backing_table: &str = row.get("backing_table");
            let type_name: &str = row.get("type_name");
            let desc = ExistingObject::new(type_name, backing_table, type_id)?;
            let ts = type_systems
                .entry(desc.version_id())
                .or_insert_with(|| TypeSystem::new(builtin.clone(), desc.version_id()));

            let fields = self.load_type_fields(ts, type_id).await?;
            let indexes = self.load_type_indexes(type_id, backing_table).await?;
            let ty = ObjectType::new(&desc, fields, indexes)?;
            ts.add_custom_type(Entity::Custom {
                object: Arc::new(ty),
                policy: None,
            })?;
        }

        Ok(type_systems)
    }

    async fn load_type_fields(&self, ts: &TypeSystem, type_id: i32) -> Result<Vec<Field>> {
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
        let rows = fetch_all(&self.db.pool, query).await?;

        let mut fields = Vec::new();
        for row in rows {
            let db_field_name: &str = row.get("field_name");
            let field_id: i32 = row.get("field_id");
            let field_type: &str = row.get("field_type");

            let split: Vec<&str> = db_field_name.split('.').collect();
            anyhow::ensure!(split.len() == 3, "Expected version and type information as part of the field name. Got {}. Database corrupted?", db_field_name);
            let field_name = split[2].to_owned();
            let version_id = split[0].to_owned();
            let desc = ExistingField::new(
                &field_name,
                ts.lookup_type(field_type)?,
                field_id,
                &version_id,
            );

            let field_def: Option<String> = row.get("default_value");
            let is_optional: bool = row.get("is_optional");
            let is_unique: bool = row.get("is_unique");

            let labels_query =
                sqlx::query("SELECT label_name FROM field_labels WHERE field_id = $1");

            let query = labels_query.bind(field_id);

            let rows = fetch_all(&self.db.pool, query).await?;

            let labels = rows
                .iter()
                .map(|r| r.get("label_name"))
                .collect::<Vec<String>>();

            fields.push(Field::new(&desc, labels, field_def, is_optional, is_unique));
        }
        Ok(fields)
    }

    async fn load_type_indexes(&self, type_id: i32, backing_table: &str) -> Result<Vec<DbIndex>> {
        let query = sqlx::query(
            r#"
            SELECT
                index_id,
                fields
            FROM indexes
            WHERE type_id = $1"#,
        )
        .bind(type_id);
        let rows = fetch_all(&self.db.pool, query).await?;

        let mut indexes = vec![];
        for row in rows {
            let index_id: i32 = row.get("index_id");
            // FIXME: bind fields to fields table.
            let fields: &str = row.get("fields");
            let fields = fields.split(';').map(|s| s.to_string()).collect();
            indexes.push(DbIndex::new(index_id, backing_table.to_owned(), fields));
        }
        Ok(indexes)
    }

    pub async fn remove_type(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> Result<()> {
        let type_id = ty
            .meta_id
            .context("logical error. Trying to delete type without id")?;

        for field in ty.user_fields() {
            remove_field_query(transaction, field).await?;
        }

        let del_type = sqlx::query("DELETE FROM types WHERE type_id = $1").bind(type_id);
        let del_type_name = sqlx::query("DELETE FROM type_names WHERE type_id = $1").bind(type_id);

        execute(transaction, del_type).await?;
        execute(transaction, del_type_name).await?;

        Ok(())
    }

    pub async fn update_type(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
        delta: ObjectDelta,
    ) -> Result<()> {
        for field in delta.added_fields.iter() {
            insert_field_query(transaction, ty, None, field).await?;
        }

        for field in delta.removed_fields.iter() {
            remove_field_query(transaction, field).await?;
        }

        for field in delta.updated_fields.iter() {
            update_field_query(transaction, field).await?;
        }

        Self::delete_indexes(transaction, &delta.removed_indexes).await?;

        Self::insert_indexes(
            transaction,
            ty.meta_id
                .context("object must have an id when it's being updated")?,
            &delta.added_indexes,
        )
        .await?;
        Ok(())
    }

    pub async fn begin_transaction(&self) -> Result<Transaction<'_, Any>> {
        Ok(self.db.pool.begin().await?)
    }

    pub async fn commit_transaction(transaction: Transaction<'_, Any>) -> Result<()> {
        transaction.commit().await?;
        Ok(())
    }

    /// Persist a specific policy version.
    ///
    /// We don't have a method that persist all policies, for all versions, because
    /// versions are applied independently
    pub async fn persist_policy_version(
        &self,
        transaction: &mut Transaction<'_, Any>,
        version_id: &str,
        policy: &str,
    ) -> Result<()> {
        let add_policy = sqlx::query(
            r#"
            INSERT INTO policies (policy_str, version)
            VALUES ($1, $2)
            ON CONFLICT(version) DO UPDATE SET policy_str = $1
            WHERE policies.version = $2"#,
        )
        .bind(policy.to_owned())
        .bind(version_id.to_owned());
        execute(transaction, add_policy).await?;
        Ok(())
    }

    pub async fn delete_policy_version(
        &self,
        transaction: &mut Transaction<'_, Any>,
        version_id: &str,
    ) -> Result<()> {
        let delete_policy =
            sqlx::query("DELETE FROM policies WHERE version = $1").bind(version_id.to_owned());
        execute(transaction, delete_policy).await?;
        Ok(())
    }

    /// Loads policy system for a version.
    ///
    /// Useful on startup, when we have to populate our in-memory state from the meta database.
    pub async fn load_policy_system(&self, version_id: &str) -> Result<PolicySystem> {
        let get_policy =
            sqlx::query("SELECT policy_str FROM policies WHERE version = $1").bind(version_id);
        let mut transaction = self.begin_transaction().await?;
        let row = fetch_one(&mut transaction, get_policy).await?;
        let yaml: &str = row.get("policy_str");
        PolicySystem::from_yaml(yaml)
    }

    pub(crate) async fn count_rows(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> anyhow::Result<i64> {
        let query = format!("SELECT COUNT(*) as count from \"{}\"", ty.backing_table());
        let count = sqlx::query(&query);
        let row = fetch_one(transaction, count).await?;
        let cnt: i64 = row.get("count");
        Ok(cnt)
    }

    pub async fn insert_type(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> Result<()> {
        let add_type = sqlx::query("INSERT INTO types (backing_table) VALUES ($1) RETURNING *");
        let add_type_name = sqlx::query("INSERT INTO type_names (type_id, name) VALUES ($1, $2)");

        let add_type = add_type.bind(ty.backing_table().to_owned());
        let row = fetch_one(transaction, add_type).await?;

        let id: i32 = row.get("type_id");
        let add_type_name = add_type_name.bind(id).bind(ty.persisted_name());
        execute(transaction, add_type_name).await?;

        for field in ty.user_fields() {
            insert_field_query(transaction, ty, Some(id), field).await?;
        }
        Self::insert_indexes(transaction, id, ty.indexes()).await?;
        Ok(())
    }

    async fn insert_indexes(
        transaction: &mut Transaction<'_, Any>,
        type_id: i32,
        indexes: &[DbIndex],
    ) -> Result<()> {
        for index in indexes {
            let fields = index.fields.join(";");
            let add_index = sqlx::query("INSERT INTO indexes (type_id, fields) VALUES ($1, $2)")
                .bind(type_id)
                .bind(fields);
            execute(transaction, add_index).await?;
        }
        Ok(())
    }

    async fn delete_indexes(
        transaction: &mut Transaction<'_, Any>,
        indexes: &[DbIndex],
    ) -> Result<()> {
        for index in indexes {
            let index_id = index
                .meta_id
                .context("index id must be known when updating type")?;
            let del_index = sqlx::query("DELETE FROM indexes WHERE index_id = $1").bind(index_id);
            execute(transaction, del_index).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datastore::{query::tests::*, QueryEngine};
    use tempdir::TempDir;

    #[tokio::test]
    // test that we can join a split meta and data db into one
    async fn migrate_split_db() -> Result<()> {
        let tmp_dir = TempDir::new("migrate_split_db")?;

        let meta_path = tmp_dir.path().join("chisel-meta.db");
        fs::copy("./test_files/split_db/chiseld-old-meta.db", &meta_path)
            .await
            .unwrap();

        let data_path = tmp_dir.path().join("chisel-data.db");
        fs::copy("./test_files/split_db/chiseld-old-data.db", &data_path)
            .await
            .unwrap();

        let new_path = tmp_dir.path().join("chisel.db");
        let conn_str = format!("sqlite://{}?mode=rwc", new_path.display());

        let conn = Arc::new(DbConnection::connect(&conn_str, 1).await?);
        let meta = MetaService::new(conn.clone());
        meta.maybe_migrate_split_sqlite_database(
            &[meta_path, data_path],
            &new_path.display().to_string(),
        )
        .await
        .unwrap();

        let query = QueryEngine::new(conn);

        let mut tss = meta
            .load_type_systems(&Arc::new(BuiltinTypes::new()))
            .await
            .unwrap();
        let ts = tss.remove("dev").unwrap();
        let ty = ts.lookup_custom_type("BlogComment").unwrap();
        let rows = fetch_rows(&query, &ty).await;
        assert_eq!(rows.len(), 10);
        Ok(())
    }

    #[tokio::test]
    async fn migrate_split_db_missing_file() -> Result<()> {
        let tmp_dir = TempDir::new("migrate_split_db_missing_files")?;

        let data_path = tmp_dir.path().join("chisel-data.db");
        let meta_path = tmp_dir.path().join("chisel-meta.db");
        fs::copy("./test_files/split_db/chiseld-old-meta.db", &meta_path)
            .await
            .unwrap();

        let new_path = tmp_dir.path().join("chisel.db");
        let conn_str = format!("sqlite://{}?mode=rwc", new_path.display());

        let conn = Arc::new(DbConnection::connect(&conn_str, 1).await?);
        let meta = MetaService::new(conn.clone());
        meta.maybe_migrate_split_sqlite_database(
            &[meta_path.clone(), data_path],
            &new_path.display().to_string(),
        )
        .await
        .unwrap_err();

        // still exists, wasn't deleted
        fs::metadata(meta_path).await.unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn migrate_split_db_bad_file() -> Result<()> {
        let tmp_dir = TempDir::new("migrate_split_db_bad_file")?;

        // duplicated entries should cause the migration to fail (because we're forcing those files
        // to be the same)
        let data_path = tmp_dir.path().join("chisel-meta.db");
        let meta_path = tmp_dir.path().join("chisel-meta.db");
        fs::copy("./test_files/split_db/chiseld-old-meta.db", &meta_path)
            .await
            .unwrap();

        let new_path = tmp_dir.path().join("chisel.db");
        let conn_str = format!("sqlite://{}?mode=rwc", new_path.display());

        let conn = Arc::new(DbConnection::connect(&conn_str, 1).await?);
        let meta = MetaService::new(conn.clone());
        meta.maybe_migrate_split_sqlite_database(
            &[meta_path.clone(), data_path.clone()],
            &new_path.display().to_string(),
        )
        .await
        .unwrap_err();

        // original still exists, werent't deleted
        fs::metadata(data_path).await.unwrap();
        fs::metadata(meta_path).await.unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn migrate_split_db_untouched() -> Result<()> {
        let tmp_dir = TempDir::new("migrate_split_db_untouched")?;

        let data_path = tmp_dir.path().join("chisel-data.db");
        fs::copy("./test_files/split_db/chiseld-old-data.db", &data_path)
            .await
            .unwrap();

        let meta_path = tmp_dir.path().join("chisel-meta.db");
        fs::copy("./test_files/split_db/chiseld-old-meta.db", &meta_path)
            .await
            .unwrap();

        let new_path = tmp_dir.path().join("chisel-new.db");
        fs::copy("./test_files/split_db/chiseld-old-meta.db", &new_path)
            .await
            .unwrap();

        // meta db has data, won't migrate. This shouldn't trigger an error because this is
        // the path we take on most boots after migration
        let conn_str = format!("sqlite://{}?mode=rwc", meta_path.display());

        let conn = Arc::new(DbConnection::connect(&conn_str, 1).await?);
        let meta = MetaService::new(conn.clone());
        meta.maybe_migrate_split_sqlite_database(
            &[meta_path.clone(), data_path.clone()],
            &new_path.display().to_string(),
        )
        .await
        .unwrap();

        // original still exists, werent't deleted
        fs::metadata(data_path).await.unwrap();
        fs::metadata(meta_path).await.unwrap();
        Ok(())
    }
}
