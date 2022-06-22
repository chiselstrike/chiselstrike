pub(crate) mod schema;

// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiInfo, ApiInfoMap};
use crate::datastore::{DbConnection, Kind};
use crate::policies::Policies;
use crate::prefix_map::PrefixMap;
use crate::types::{
    DbIndex, Entity, ExistingField, ExistingObject, Field, FieldDelta, ObjectDelta, ObjectType,
    Type, TypeSystem,
};
use anyhow::Context;
use sqlx::any::{Any, AnyPool};
use sqlx::{Execute, Executor, Row, Transaction};
use std::fmt::Write;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

/// Meta service.
///
/// The meta service is responsible for managing metadata such as object
/// types and labels persistently.
#[derive(Debug)]
pub(crate) struct MetaService {
    kind: Kind,
    pool: AnyPool,
}

async fn execute<'a, 'b>(
    transaction: &mut Transaction<'b, sqlx::Any>,
    query: sqlx::query::Query<'a, sqlx::Any, sqlx::any::AnyArguments<'a>>,
) -> anyhow::Result<sqlx::any::AnyQueryResult> {
    let qstr = query.sql();
    transaction
        .execute(query)
        .await
        .with_context(|| format!("Failed to execute query {}", qstr))
}

async fn fetch_one<'a, 'b>(
    transaction: &mut Transaction<'b, sqlx::Any>,
    query: sqlx::query::Query<'a, sqlx::Any, sqlx::any::AnyArguments<'a>>,
) -> anyhow::Result<sqlx::any::AnyRow> {
    let qstr = query.sql();
    transaction
        .fetch_one(query)
        .await
        .with_context(|| format!("Failed to execute query {}", qstr))
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
        .with_context(|| format!("Failed to execute query {}", qstr))
}

async fn file_exists(file: &Path) -> anyhow::Result<bool> {
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

async fn remove_field_query(
    transaction: &mut Transaction<'_, Any>,
    field: &Field,
) -> anyhow::Result<()> {
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
) -> anyhow::Result<()> {
    let type_id = ty.meta_id.xor(recently_added_type_id).context(
        "logical error. Seems like a type is at the same type pre-existing and recently added??",
    )?;

    let add_field = match &field.user_provided_default() {
        None => {
            let query = sqlx::query(
                r#"
                INSERT INTO fields (field_type, type_id, is_optional, is_unique, junction_table)
                VALUES ($1, $2, $3, $4, $5)
                RETURNING *"#,
            );
            query
                .bind(field.type_id.name())
                .bind(type_id)
                .bind(field.is_optional)
                .bind(field.is_unique)
                .bind(field.junction_table.to_owned())
        }
        Some(value) => {
            let query = sqlx::query(
                r#"
                INSERT INTO fields (
                    field_type,
                    type_id,
                    default_value,
                    is_optional,
                    is_unique,
                    junction_table)
                VALUES ($1, $2, $3, $4, $5, $6)
                RETURNING *"#,
            );
            query
                .bind(field.type_id.name())
                .bind(type_id)
                .bind(value.to_owned())
                .bind(field.is_optional)
                .bind(field.is_unique)
                .bind(field.junction_table.to_owned())
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

    /// Try to migrate an old-style (split meta + data) sqlite layout to
    /// a single file.
    ///
    /// For Postgres this is a lot more complex because it is not possible to
    /// do cross-database transactions. But this handles the local dev case,
    /// which is what is most relevant at the moment.
    pub(crate) async fn maybe_migrate_sqlite_database<P: AsRef<Path>, T: AsRef<Path>>(
        &self,
        sources: &[P],
        to: T,
    ) -> anyhow::Result<()> {
        let to = to.as_ref();
        match self.kind {
            Kind::Sqlite => {}
            _ => anyhow::bail!("Can't migrate postgres tables yet"),
        }

        // For the old files, either none of them exists (in which case
        // we're ok), or all of them exists. The case in which some of them
        // exists we're better off not touching, and erroring out.
        let mut not_found = vec![];
        for src in sources {
            let src = src.as_ref();
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

        let mut transaction = self.start_transaction().await?;
        match self
            .maybe_migrate_sqlite_database_inner(&mut transaction, sources)
            .await
        {
            Err(x) => Err(x),
            Ok(true) => {
                let mut full_str = format!(
                    "Migrated your data to {}. You can now delete the following files:",
                    to.display()
                );
                for src in sources {
                    let s = src.as_ref().display();
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

    async fn maybe_migrate_sqlite_database_inner<P: AsRef<Path>>(
        &self,
        transaction: &mut Transaction<'_, Any>,
        sources: &[P],
    ) -> anyhow::Result<bool> {
        // this sqlite instance already has data, nothing to migrate.
        if self.count_tables(transaction).await? != 0 {
            return Ok(false);
        }

        for (idx, src) in sources.iter().enumerate() {
            let src = src.as_ref().to_str().unwrap();
            let db = format!("db{}", idx);

            let attach = format!("attach database '{}' as '{}'", src, db);
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
            execute(&mut transaction, query).await?;
        }

        if new_install {
            let query =
                sqlx::query("INSERT INTO chisel_version (version, version_id) VALUES ($1, $2)")
                    .bind(schema::CURRENT_VERSION)
                    .bind("chiselstrike");
            execute(&mut transaction, query).await?;
        }

        let mut version = Self::get_version(&mut transaction).await?;

        while version != schema::CURRENT_VERSION {
            let (statements, new_version) = schema::evolve_from(&version).await?;

            for statement in statements {
                let query = statement.build_any(query_builder);
                let query = sqlx::query(&query);
                execute(&mut transaction, query).await?;
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

            execute(&mut transaction, query).await?;
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
        execute(transaction, add_api).await?;
        Ok(())
    }

    /// Load the existing endpoints from from metadata store.
    pub(crate) async fn load_sources<'r>(&self) -> anyhow::Result<PrefixMap<String>> {
        let query = sqlx::query("SELECT path, code FROM sources");
        let rows = fetch_all(&self.pool, query).await?;

        let mut sources = PrefixMap::default();
        for row in rows {
            let path: &str = row.get("path");
            let code: &str = row.get("code");
            debug!("Loading source {}", path);
            sources.insert(path.into(), code.to_string());
        }
        Ok(sources)
    }

    pub(crate) async fn persist_sources(&self, sources: &PrefixMap<String>) -> anyhow::Result<()> {
        let mut transaction = self.pool.begin().await?;

        let drop = sqlx::query("DELETE FROM sources");
        execute(&mut transaction, drop).await?;

        for (path, code) in sources.iter() {
            let new_route = sqlx::query("INSERT INTO sources (path, code) VALUES ($1, $2)")
                .bind(path.to_str())
                .bind(code);

            execute(&mut transaction, new_route).await?;
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
            let indexes = self.load_type_indexes(type_id, backing_table).await?;

            let ty = ObjectType::new(desc, fields, indexes)?;
            ts.add_custom_type(Entity::Custom(Arc::new(ty)))?;
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
                fields.is_unique AS is_unique,
                fields.junction_table AS junction_table
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
            let junction_table: Option<&str> = row.get("junction_table");

            let split: Vec<&str> = db_field_name.split('.').collect();
            anyhow::ensure!(split.len() == 3, "Expected version and type information as part of the field name. Got {}. Database corrupted?", db_field_name);
            let field_name = split[2].to_owned();
            let version = split[0].to_owned();

            let field_type = {
                // FIXME: The type should be stored as a JSON or some structured data.
                let re = regex::Regex::new("^List<(.*)>$").unwrap();
                if let Some(caps) = re.captures(field_type) {
                    let entity_name = caps.get(1).unwrap().as_str();
                    Type::List(ts.lookup_entity(entity_name, &version)?)
                } else {
                    ts.lookup_type(field_type, &version)?
                }
            };

            let desc =
                ExistingField::new(&field_name, field_type, field_id, &version, junction_table);
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

    async fn load_type_indexes(
        &self,
        type_id: i32,
        backing_table: &str,
    ) -> anyhow::Result<Vec<DbIndex>> {
        let query = sqlx::query(
            r#"
            SELECT
                index_id,
                fields
            FROM indexes
            WHERE type_id = $1"#,
        )
        .bind(type_id);
        let rows = fetch_all(&self.pool, query).await?;

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

        execute(transaction, del_type).await?;
        execute(transaction, del_type_name).await?;

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
        execute(transaction, add_policy).await?;
        Ok(())
    }

    pub(crate) async fn delete_policy_version(
        &self,
        transaction: &mut Transaction<'_, Any>,
        version: &str,
    ) -> anyhow::Result<()> {
        let delete_policy =
            sqlx::query("DELETE FROM policies WHERE version = $1").bind(version.to_owned());
        execute(transaction, delete_policy).await?;
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
    ) -> anyhow::Result<()> {
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
    ) -> anyhow::Result<()> {
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

        let conn = DbConnection::connect(&conn_str, 1).await?;
        let meta = MetaService::local_connection(&conn, 1).await.unwrap();
        meta.maybe_migrate_sqlite_database(&[&meta_path, &data_path], &new_path)
            .await
            .unwrap();

        let query = QueryEngine::local_connection(&conn, 1).await.unwrap();

        let ts = meta.load_type_system().await.unwrap();
        let ty = ts.lookup_custom_type("BlogComment", "dev").unwrap();
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

        let conn = DbConnection::connect(&conn_str, 1).await?;
        let meta = MetaService::local_connection(&conn, 1).await.unwrap();
        meta.maybe_migrate_sqlite_database(&[&meta_path, &data_path], &new_path)
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

        let conn = DbConnection::connect(&conn_str, 1).await?;
        let meta = MetaService::local_connection(&conn, 1).await.unwrap();
        meta.maybe_migrate_sqlite_database(&[&meta_path, &data_path], &new_path)
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

        let conn = DbConnection::connect(&conn_str, 1).await?;
        let meta = MetaService::local_connection(&conn, 1).await.unwrap();
        meta.maybe_migrate_sqlite_database(&[&meta_path, &data_path], &new_path)
            .await
            .unwrap();

        // original still exists, werent't deleted
        fs::metadata(data_path).await.unwrap();
        fs::metadata(meta_path).await.unwrap();
        Ok(())
    }
}
