// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::{anyhow, Context as AnyhowContext, Result};
use async_lock::{Mutex, MutexGuardArc};
use deno_core::futures;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::FutureExt;
use futures::StreamExt;
use itertools::Itertools;
use pin_project::pin_project;
use sea_query::{Alias, ColumnDef, Index, PostgresQueryBuilder, Table};
use serde::Serialize;
use sqlx::any::{Any, AnyArguments, AnyKind, AnyRow};
use sqlx::{Executor, Row, Transaction, ValueRef};
use uuid::Uuid;

use crate::datastore::query::{
    KeepOrOmitField, Mutation, QueriedEntity, QueryField, QueryPlan, SqlValue, TargetDatabase,
};
use crate::datastore::value::{EntityMap, EntityValue};
use crate::datastore::DbConnection;
use crate::ops::job_context::JobInfo;
use crate::policies::PolicySystem;
use crate::types::{DbIndex, Field, ObjectDelta, ObjectType, Type, TypeId, TypeSystem};

use super::DataContext;

/// A query results is a stream of query rows after policies have been applied.
pub type QueryResults = BoxStream<'static, Result<EntityMap>>;

pub type TransactionStatic = Arc<Mutex<Transaction<'static, Any>>>;

pub fn extract_transaction(transaction: TransactionStatic) -> Transaction<'static, Any> {
    let transaction = Arc::try_unwrap(transaction).expect("Transaction still has references held!");
    transaction.into_inner()
}

/// `RawQueryResults` represents the raw query results from the backing stor
///  before policies are applied.
#[pin_project]
struct RawQueryResults<T> {
    raw_query: String,
    tr: MutexGuardArc<Transaction<'static, Any>>,
    #[pin]
    stream: T,
}

async fn make_transactioned_stream(
    tr: TransactionStatic,
    raw_query: String,
) -> impl Stream<Item = anyhow::Result<AnyRow>> {
    let mut tr = tr.lock_arc().await;

    // The string data and Transaction will not move anymore.
    let raw_query_ptr = raw_query.as_ref() as *const str;
    let query = sqlx::query::<Any>(unsafe { &*raw_query_ptr });
    let tr_ptr = &mut *tr as *mut _;
    let tr_ref = unsafe { &mut *tr_ptr };
    let stream = query.fetch(tr_ref).map(|i| i.map_err(anyhow::Error::new));

    RawQueryResults {
        tr,
        raw_query,
        stream,
    }
}

fn new_query_results(
    raw_query: String,
    tr: TransactionStatic,
) -> impl Stream<Item = anyhow::Result<AnyRow>> {
    make_transactioned_stream(tr, raw_query).flatten_stream()
}

impl<T: Stream<Item = Result<AnyRow>>> Stream for RawQueryResults<T> {
    type Item = Result<AnyRow>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }
}

impl TryFrom<&Field> for ColumnDef {
    type Error = anyhow::Error;
    fn try_from(field: &Field) -> Result<Self> {
        let mut column_def = ColumnDef::new(Alias::new(&field.name));
        if field.is_unique {
            column_def.unique_key();
        }
        match field.type_id {
            TypeId::String => column_def.text(),
            TypeId::Id => column_def.text().primary_key(),
            TypeId::Float | TypeId::JsDate => column_def.double(),
            TypeId::Boolean => column_def.boolean(),
            TypeId::Entity { .. } => column_def.text(), // Foreign key, must the be same type as Type::Id
            TypeId::Array(_) => column_def.text(),      // Arrays are stored as serialized JSONs.
        };

        Ok(column_def)
    }
}

/// An SQL string with placeholders, plus its argument values.  Keeps them all alive so they can be fed to
/// sqlx::Query by reference.
#[derive(Debug)]
pub struct SqlWithArguments {
    /// SQL query text with placeholders $1, $2, ...
    pub sql: String,
    /// Values for $n placeholders.
    pub args: Vec<SqlValue>,
}

impl SqlWithArguments {
    fn get_sqlx(&self) -> sqlx::query::Query<'_, sqlx::Any, AnyArguments> {
        let mut sqlx_query = sqlx::query(&self.sql);
        for arg in &self.args {
            match arg {
                SqlValue::Bool(arg) => sqlx_query = sqlx_query.bind(arg),
                SqlValue::F64(arg) => sqlx_query = sqlx_query.bind(arg),
                SqlValue::String(arg) => sqlx_query = sqlx_query.bind(arg),
            };
        }
        sqlx_query
    }
}

/// Represents recurent structure of nested object ids. Each level holds
/// the `id` of the current object and `children` object in a map where
/// key is the field name of the object and value is another IdTree level.
///
/// Example: Object {
///     id: "xxx",
///     foo: 1,
///     bar: {
///         id: "yyy",
///         count: 12
///     }
/// } will yield an ID tree: {
///     id: "xxx",
///     children: {
///         bar: {
///             id: "yyy"
///         }
///     }
/// }
#[derive(Debug, Serialize)]
pub struct IdTree {
    pub id: String,
    children: HashMap<String, IdTree>,
}

fn column_is_null(row: &AnyRow, column_idx: usize) -> bool {
    row.try_get_raw(column_idx).unwrap().is_null()
}

fn id_idx(entity: &QueriedEntity) -> usize {
    for f in &entity.fields {
        match f {
            QueryField::Scalar {
                name, column_idx, ..
            } if name == "id" => return *column_idx,
            _ => (),
        }
    }
    panic!("No id field among Entity children");
}

/// Query engine.
///
/// The query engine provides a way to transactionally mutate entities and
/// retrieve them from a backing store for ChiselStrike endpoints.
///
/// The query engine works on `Mutation` and `Query` objects that represent
/// how to mutate the underlying backing store or how to retrieve entities.
/// The query engine attempts to perform as much of the query logic in the
/// backing store database to take advantage of the query optimizer. However,
/// some parts of a mutation or query need to run through the policy engine,
/// which is not always offloadable to a database.
#[derive(Clone)]
pub struct QueryEngine {
    db: Arc<DbConnection>,
}

impl QueryEngine {
    pub fn new(db: Arc<DbConnection>) -> Self {
        Self { db }
    }

    fn target_db(&self) -> TargetDatabase {
        match self.db.pool.any_kind() {
            AnyKind::Postgres => TargetDatabase::Postgres,
            AnyKind::Sqlite => TargetDatabase::Sqlite,
        }
    }

    pub async fn drop_table(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> Result<()> {
        self.drop_indexes(transaction, ty, ty.indexes()).await?;

        let drop_table = Table::drop()
            .table(Alias::new(ty.backing_table()))
            .to_owned();
        let drop_table = drop_table.build_any(self.db.schema_builder());
        let drop_table = sqlx::query(&drop_table);
        transaction.execute(drop_table).await?;

        Ok(())
    }

    pub async fn begin_transaction_static(&self) -> Result<TransactionStatic> {
        Ok(Arc::new(Mutex::new(self.db.pool.begin().await?)))
    }

    pub async fn create_data_context(
        &self,
        type_system: Arc<TypeSystem>,
        policy_system: Arc<PolicySystem>,
        job_info: Rc<JobInfo>,
    ) -> Result<DataContext> {
        let txn = self.begin_transaction_static().await?;
        Ok(DataContext {
            type_system,
            policy_system,
            txn,
            job_info,
        })
    }

    pub async fn begin_transaction(&self) -> Result<Transaction<'static, Any>> {
        Ok(self.db.pool.begin().await?)
    }

    pub async fn commit_transaction(transaction: Transaction<'static, Any>) -> Result<()> {
        transaction.commit().await?;
        Ok(())
    }

    pub async fn commit_transaction_static(transaction: TransactionStatic) -> Result<()> {
        let transaction = extract_transaction(transaction);
        transaction.commit().await?;
        Ok(())
    }

    pub async fn create_table(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> Result<()> {
        let mut create_table = Table::create()
            .table(Alias::new(ty.backing_table()))
            .if_not_exists()
            .to_owned();

        for field in ty.all_fields() {
            let mut column_def = ColumnDef::try_from(field)?;
            create_table.col(&mut column_def);
        }
        let create_table = create_table.build_any(self.db.schema_builder());

        let create_table = sqlx::query(&create_table);
        transaction.execute(create_table).await?;

        Self::create_indexes(transaction, ty, ty.indexes()).await?;
        Ok(())
    }

    pub async fn alter_table(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
        delta: ObjectDelta,
    ) -> Result<()> {
        self.drop_indexes(transaction, ty, &delta.removed_indexes)
            .await?;

        // using a macro as async closures are unstable
        macro_rules! do_query {
            ( $table:expr ) => {{
                let table = $table.build_any(&PostgresQueryBuilder);
                let table = sqlx::query(&table);

                transaction.execute(table).await
            }};
        }

        // SQLite doesn't support multiple add column statements
        // (details at https://github.com/SeaQL/sea-query/issues/213), generate a separate alter
        // statement for each delta
        //
        // Alter table is also problematic on SQLite for other reasons (https://sqlite.org/lang_altertable.html)
        //
        // However there are some modifications that are safe (like adding a column or removing a
        // non-foreign-key column), but sqlx doesn't even generate the statement for them.
        //
        // So we fake being Postgres. Our ALTERs should be well-behaved, but we then need to make
        // sure we're not doing any kind of operation that are listed among the problematic ones.
        //
        // In particular, we can't use defaults, which is fine since we can handle that on
        // chiselstrike's side.
        //
        // FIXME: When we start generating indexes or using foreign keys, we'll have to make sure
        // that those are still safe. Adding columns is always safe, but removals may not be if
        // they are used in relations or indexes (see the document above)
        for field in delta.added_fields.iter() {
            let mut column_def = ColumnDef::try_from(field)?;
            let table = Table::alter()
                .table(Alias::new(ty.backing_table()))
                .add_column(&mut column_def)
                .to_owned();

            do_query!(table)?;
        }

        for field in delta.removed_fields.iter() {
            let table = Table::alter()
                .table(Alias::new(ty.backing_table()))
                .drop_column(Alias::new(&field.name))
                .to_owned();

            do_query!(table)?;
        }
        // We don't loop over the modified part of the delta: SQLite doesn't support modify columns
        // at all, but that is fine since the currently supported field modifications are handled
        // by ChiselStrike directly and require no modifications to the tables.
        //
        // There are modifications that we can accept on application side (like changing defaults),
        // since we always write with defaults. For all others, we should error out way before we
        // get here.

        Self::create_indexes(transaction, ty, ty.indexes()).await?;

        Ok(())
    }

    pub async fn create_indexes(
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
        indexes: &[DbIndex],
    ) -> Result<()> {
        for index in indexes {
            let idx_name = index
                .name()
                .context("index must have a name at a time of table creation")?;
            let columns = index.fields.iter().join(", ");
            let create_index = format!(
                r#"
                CREATE INDEX IF NOT EXISTS "{idx_name}" ON "{}" ({columns});
            "#,
                ty.backing_table()
            );
            let create_index = sqlx::query(&create_index);
            transaction.execute(create_index).await?;
        }
        Ok(())
    }

    pub async fn drop_indexes(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
        indexes: &[DbIndex],
    ) -> Result<()> {
        for removed_idx in indexes {
            let drop_idx = Index::drop()
                .name(
                    &removed_idx
                        .name()
                        .context("index must have a name when dropped")?,
                )
                .table(Alias::new(ty.backing_table()))
                .to_owned();

            let drop_idx = drop_idx.build_any(&PostgresQueryBuilder);
            let drop_idx = sqlx::query(&drop_idx);
            transaction.execute(drop_idx).await?;
        }
        Ok(())
    }

    fn row_to_entity_value(
        db_kind: AnyKind,
        entity: &QueriedEntity,
        row: &AnyRow,
    ) -> Result<EntityMap> {
        let mut ret = EntityMap::default();
        for s_field in &entity.fields {
            match s_field {
                QueryField::Scalar {
                    name,
                    type_id,
                    column_idx,
                    is_optional,
                    transform,
                    keep_or_omit,
                    ..
                } => {
                    let omit_field = matches!(keep_or_omit, KeepOrOmitField::Omit);
                    if omit_field || (*is_optional && column_is_null(row, *column_idx)) {
                        continue;
                    }
                    let mut val = match type_id {
                        TypeId::Float => {
                            // https://github.com/launchbadge/sqlx/issues/1596
                            // sqlx gets confused if the float doesn't have decimal points.
                            let val: f64 = row.get_unchecked(column_idx);
                            EntityValue::Float64(val)
                        }
                        TypeId::JsDate => {
                            let val: f64 = row.get_unchecked(column_idx);
                            EntityValue::JsDate(val)
                        }
                        TypeId::String | TypeId::Id => {
                            let val = row.get::<&str, _>(column_idx);
                            EntityValue::String(val.to_owned())
                        }
                        TypeId::Boolean => {
                            // Similarly to the float issue, type information is not filled in
                            // *if* this value was put in as a result of coalesce() (default).
                            let v = match db_kind {
                                AnyKind::Sqlite => {
                                    let val: String = row.get_unchecked(column_idx);
                                    val == "1" || val.to_lowercase() == "true"
                                }
                                _ => row.get::<bool, _>(column_idx),
                            };
                            EntityValue::Boolean(v)
                        }
                        TypeId::Entity { .. } => anyhow::bail!("object is not a scalar"),
                        TypeId::Array(_) => {
                            let array_str = row.get::<&str, _>(column_idx);
                            serde_json::from_str::<EntityValue>(array_str)
                                .context("failed to deserialize array from raw JSON string")?
                        }
                    };
                    if let Some(tr) = transform {
                        // Apply policy transformation
                        val = tr(val);
                    }
                    ret.insert(name.clone(), val);
                }
                QueryField::Entity {
                    name,
                    is_optional,
                    transform,
                    keep_or_omit,
                } => {
                    let omit_field = matches!(keep_or_omit, KeepOrOmitField::Omit);
                    let child_entity = entity.get_child_entity(name).unwrap();
                    if omit_field || (*is_optional && column_is_null(row, id_idx(child_entity))) {
                        continue;
                    }
                    let val = Self::row_to_entity_value(db_kind, child_entity, row)?;
                    let mut val = EntityValue::Map(val);
                    if let Some(tr) = transform {
                        // Apply policy transformation
                        val = tr(val);
                    }
                    ret.insert(name.clone(), val);
                }
            }
        }
        Ok(ret)
    }

    fn project(
        o: Result<EntityMap>,
        allowed_fields: &Option<HashSet<String>>,
    ) -> Result<EntityMap> {
        let mut o = o?;
        if let Some(allowed_fields) = &allowed_fields {
            let removed_keys = o
                .iter()
                .map(|(k, _)| k.to_owned())
                .filter(|k| !allowed_fields.contains(k))
                .collect::<Vec<String>>();
            for k in &removed_keys {
                o.remove(k);
            }
        }
        Ok(o)
    }

    /// Execute the given `query` and return a stream to the results.
    pub fn query(
        &self,
        txn: TransactionStatic,
        query_plan: QueryPlan,
    ) -> anyhow::Result<QueryResults> {
        let query = query_plan.build_query(&self.target_db())?;
        let allowed_fields = query.allowed_fields;
        let db_kind = self.db.pool.any_kind();

        let stream = new_query_results(query.raw_sql, txn);
        let stream =
            stream.map(move |row| Self::row_to_entity_value(db_kind, &query.entity, &row?));
        let stream = Box::pin(stream.map(move |o| Self::project(o, &allowed_fields)));
        Ok(stream)
    }

    pub async fn mutate_with_transaction(
        &self,
        mutation: Mutation,
        txn: &mut Transaction<'_, Any>,
    ) -> Result<()> {
        let raw_sql = mutation.build_sql(self.target_db())?;
        let query = sqlx::query(&raw_sql);
        txn.execute(query).await?;

        Ok(())
    }

    /// Inserts object of type `ty` and value `ty_value` into the database.
    /// Returns JSON containing ids of all inserted objects in the format of
    /// IdsJson = {
    ///     "id": new_object_id,
    ///     "field_object1": IdsJson,
    ///     "field_object2": IdsJson,
    ///     ...
    /// }
    ///
    pub fn add_row(
        &self,
        ty: Arc<ObjectType>,
        record: &EntityMap,
        ctx: &DataContext,
    ) -> Result<impl Future<Output = Result<IdTree>> + '_> {
        let res = self.prepare_insertion(&ty, record, &ctx.type_system);
        let txn = ctx.txn.clone();
        Ok(async move {
            let (inserts, id_tree) = res?;
            let mut txn = txn.lock().await;
            self.run_sql_queries(&inserts, &mut txn).await?;
            Ok(id_tree)
        })
    }

    pub async fn add_row_shallow(
        &self,
        txn: &mut Transaction<'_, Any>,
        ty: &ObjectType,
        fields_map: &EntityMap,
    ) -> Result<()> {
        let query = self.prepare_insertion_shallow(ty, fields_map)?;
        self.run_sql_queries(&[query], txn).await?;
        Ok(())
    }

    pub async fn fetch_one(&self, q: SqlWithArguments) -> Result<AnyRow> {
        Ok(q.get_sqlx().fetch_one(&self.db.pool).await?)
    }

    async fn run_sql_queries(
        &self,
        queries: &[SqlWithArguments],
        transaction: &mut Transaction<'_, Any>,
    ) -> Result<()> {
        for q in queries {
            transaction.execute(q.get_sqlx()).await?;
        }

        Ok(())
    }

    fn incompatible(field: &Field, ty: &ObjectType) -> anyhow::Error {
        anyhow!(
            "provided data for field `{}` are incompatible with given type `{}`",
            field.name,
            ty.name()
        )
    }

    /// Recursively generates insert SQL queries necessary to insert object of type `ty`
    /// and value `ty_value` into database.
    /// Returns vector of SQL insert queries with corresponding arguments and IdTree of
    /// inserted objects.
    fn prepare_insertion(
        &self,
        ty: &ObjectType,
        fields_map: &EntityMap,
        ts: &TypeSystem,
    ) -> Result<(Vec<SqlWithArguments>, IdTree)> {
        let mut child_ids = HashMap::<String, IdTree>::new();
        let mut obj_id = Option::<String>::None;
        let mut query_args = Vec::<SqlValue>::new();
        let mut inserts = Vec::<SqlWithArguments>::new();

        for field in ty.all_fields() {
            let field_value = fields_map.get(&field.name);
            if (field_value.is_none() || field_value.unwrap().is_null()) && field.is_optional {
                continue;
            }
            let incompatible_data = || QueryEngine::incompatible(field, ty);
            let arg = match ts.get(&field.type_id)? {
                Type::Entity(nested_type) => {
                    let nested_value = field_value
                        .context("json object doesn't have required field")
                        .with_context(incompatible_data)?
                        .as_map()
                        .context("unexpected json type (expected an object)")
                        .with_context(incompatible_data)?;

                    let nested_id = if nested_type.is_auth() {
                        match nested_value.get("id") {
                            // We could check if the nested value matches a database row, at the cost of
                            // significant code complication and slowdown.  But that still wouldn't prevent
                            // problems, as that row can be modified by another thread after our check but before
                            // this save completes.  Better to check at compilation time that the endpoint code
                            // doesn't attempt to modify auth types.
                            Some(EntityValue::String(id)) => id.clone(),
                            _ => anyhow::bail!(
                                "Cannot save into nested type {}.",
                                nested_type.name()
                            ),
                        }
                    } else {
                        let (nested_inserts, nested_ids) =
                            self.prepare_insertion(&nested_type, nested_value, ts)?;
                        inserts.extend(nested_inserts);
                        let nested_id = nested_ids.id.to_owned();
                        child_ids.insert(field.name.to_owned(), nested_ids);
                        nested_id
                    };
                    SqlValue::String(nested_id)
                }
                _ => self
                    .convert_to_argument(field, fields_map)
                    .with_context(incompatible_data)?,
            };

            if field.name == "id" {
                obj_id = Some(
                    arg.as_string()
                        .context("the id value is not string")?
                        .to_owned(),
                );
            }
            query_args.push(arg);
        }

        inserts.push(SqlWithArguments {
            sql: self.make_insert_query(ty, fields_map)?,
            args: query_args,
        });
        let obj_id = obj_id
            .ok_or_else(|| anyhow!("attempting to insert an object `{}` with no id", ty.name()))?;
        Ok((
            inserts,
            IdTree {
                id: obj_id,
                children: child_ids,
            },
        ))
    }

    /// Converts `field` with value `ty_value` into SqlValue while ensuring the
    /// generation of default and generable values.
    fn convert_to_argument(&self, field: &Field, fields: &EntityMap) -> Result<SqlValue> {
        macro_rules! parse_default_value {
            (str, $value:expr) => {{
                $value
            }};
            ($fallback:ty, $value:expr) => {{
                let value: $fallback = $value
                    .as_str()
                    .parse()
                    .context("failed to parse default value")?;
                value
            }};
        }
        macro_rules! convert_json_value {
            ($as_type:ident, $fallback:ty) => {{
                match fields.get(&field.name) {
                    Some(value_json) => value_json
                        .$as_type()
                        .context("failed to convert json to specific type")?
                        .to_owned(),
                    None => {
                        let value = field.generate_value().context("failed to generate value")?;
                        parse_default_value!($fallback, value)
                    }
                }
            }};
        }

        let arg = match &field.type_id {
            TypeId::String | TypeId::Id | TypeId::Entity { .. } => {
                SqlValue::String(convert_json_value!(as_str, String))
            }
            TypeId::Float => SqlValue::F64(convert_json_value!(as_f64, f64)),
            TypeId::JsDate => SqlValue::F64(convert_json_value!(as_date, f64)),
            TypeId::Boolean => SqlValue::Bool(convert_json_value!(as_bool, bool)),
            TypeId::Array(element_type) => {
                let val = match fields.get(&field.name) {
                    Some(field) => {
                        self.validate_array(element_type, field)
                            .context("provided value for array has invalid type")?;
                        serde_json::to_string(field)?
                    }
                    None => field.generate_value().context("failed to generate value")?,
                };
                SqlValue::String(val)
            }
        };

        Ok(arg)
    }

    /// `validate_array` ensures that given JSON `value` is an array and it's elements are of
    /// compliant type with `element_type`.
    fn validate_array(&self, element_type: &TypeId, value: &EntityValue) -> Result<()> {
        let elements = value.as_array()?;
        for (i, e) in elements.iter().enumerate() {
            macro_rules! bail {
               () => { anyhow::bail!("stored array should have elements of type '{}', but found '{:?}' at position {i}", element_type.name(), e)}
            }
            macro_rules! maybe_bail {
                ($is_type:ident) => {{
                    if !e.$is_type() {
                        bail!();
                    }
                }};
            }
            match element_type {
                TypeId::String | TypeId::Id => maybe_bail!(is_string),
                TypeId::Float => maybe_bail!(is_f64),
                TypeId::JsDate => {
                    if !e.is_date() && !e.is_f64() {
                        bail!();
                    }
                }
                TypeId::Boolean => maybe_bail!(is_boolean),
                TypeId::Array(inner_element) => self
                    .validate_array(inner_element, e)
                    .context("failed to validate inner array at position {i}")?,
                TypeId::Entity { .. } => {
                    unreachable!("entity can't be a contained within an array")
                }
            }
        }
        Ok(())
    }

    /// For given object of type `ty` and its value `ty_value` computes a string
    /// representing SQL query which inserts the object into database.
    fn make_insert_query(&self, ty: &ObjectType, fields_map: &EntityMap) -> Result<String> {
        let mut field_binds = String::new();
        let mut field_names = vec![];
        let mut id_name = String::new();
        let mut update_binds = String::new();
        let mut id_bind = String::new();

        let mut i = 0;
        for f in ty.all_fields() {
            let val = fields_map.get(&f.name);
            if val.is_none() && f.is_optional {
                continue;
            }
            let bind = if f.is_optional && val.unwrap().is_null() {
                // sqlx has trouble binding null values in some cases; insert them verbatim.
                "NULL".to_string()
            } else {
                i += 1;
                std::format!("${}", i)
            };
            field_binds.push_str(&bind);
            field_binds.push(',');

            field_names.push(f.name.clone());
            if f.type_id == TypeId::Id {
                if let Some(idstr) = val {
                    let idstr = idstr.as_str().context("invalid ID: It is not a string")?;
                    Uuid::parse_str(idstr).map_err(|_| anyhow!("invalid ID '{}'", idstr))?;
                }
                anyhow::ensure!(id_bind.is_empty(), "More than one ID??");
                id_name = f.name.to_string();
                id_bind = bind.clone();
            }
            write!(update_binds, "\"{}\" = {},", &f.name, &bind).unwrap();
        }
        field_binds.pop();
        update_binds.pop();

        for v in fields_map.keys() {
            anyhow::ensure!(
                field_names.contains(v),
                "field {} not present in {}",
                v,
                ty.name()
            );
        }

        Ok(std::format!(
            "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {} WHERE \"{}\".\"{}\" = {}",
            &ty.backing_table(),
            field_names.into_iter().map(|f| format!("\"{}\"", f)).join(","),
            field_binds,
            id_name,
            update_binds,
            &ty.backing_table(),
            id_name,
            id_bind,
        ))
    }

    fn prepare_insertion_shallow(
        &self,
        ty: &ObjectType,
        fields_map: &EntityMap,
    ) -> Result<SqlWithArguments> {
        let mut query_args = Vec::<SqlValue>::new();
        for field in ty.all_fields() {
            if fields_map.get(&field.name).is_none() && field.is_optional {
                continue;
            }
            let arg = self
                .convert_to_argument(field, fields_map)
                .with_context(|| QueryEngine::incompatible(field, ty))?;
            query_args.push(arg);
        }

        Ok(SqlWithArguments {
            sql: self.make_insert_query(ty, fields_map)?,
            args: query_args,
        })
    }
}
