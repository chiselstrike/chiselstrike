// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::db::{sql, Relation, SqlValue};
use crate::query::{DbConnection, Kind, QueryError};
use crate::types::{Field, ObjectDelta, ObjectType, Type, OAUTHUSER_TYPE_NAME};
use anyhow::{anyhow, Context as AnyhowContext};
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::StreamExt;
use itertools::{zip, Itertools};
use sea_query::{Alias, ColumnDef, Table};
use serde_json::json;
use sqlx::any::{Any, AnyPool, AnyRow};
use sqlx::Column;
use sqlx::Transaction;
use sqlx::{Executor, Row};
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};
use uuid::Uuid;

// Results directly out of the database
pub(crate) type RawSqlStream = BoxStream<'static, anyhow::Result<AnyRow>>;

// Results with policies applied
pub(crate) type JsonObject = serde_json::Map<String, serde_json::Value>;
pub(crate) type SqlStream = BoxStream<'static, anyhow::Result<JsonObject>>;

struct QueryResults {
    raw_query: String,
    // The streams we use in here only depend on the lifetime of raw_query.
    stream: RefCell<Option<RawSqlStream>>,
    _marker: PhantomPinned, // QueryStream cannot be moved
}

pub(crate) fn new_query_results(raw_query: String, pool: &AnyPool) -> RawSqlStream {
    let ret = Box::pin(QueryResults {
        raw_query,
        stream: RefCell::new(None),
        _marker: PhantomPinned,
    });
    let ptr: *const String = &ret.raw_query;
    let query = sqlx::query::<Any>(unsafe { &*ptr });
    let stream = query.fetch(pool).map(|i| i.map_err(anyhow::Error::new));
    ret.stream.replace(Some(Box::pin(stream)));
    ret
}

impl Stream for QueryResults {
    type Item = anyhow::Result<AnyRow>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut borrow = self.stream.borrow_mut();
        borrow.as_mut().unwrap().as_mut().poll_next(cx)
    }
}

impl TryFrom<&Field> for ColumnDef {
    type Error = anyhow::Error;
    fn try_from(field: &Field) -> anyhow::Result<Self> {
        let mut column_def = ColumnDef::new(Alias::new(&field.name));
        match field.type_ {
            Type::String => column_def.text(),
            Type::Id => column_def.text().unique_key().primary_key(),
            Type::Float => column_def.double(),
            Type::Boolean => column_def.integer(),
            Type::Object(_) => column_def.text(), // Foreign key, must the be same type as Type::Id
        };

        Ok(column_def)
    }
}

/// An SQL string with placeholders, plus its argument values.  Keeps them all alive so they can be fed to
/// sqlx::Query by reference.
#[derive(Debug)]
struct SqlWithArguments {
    /// SQL query text with placeholders $1, $2, ...
    sql: String,
    /// Values for $n placeholders.
    args: Vec<SqlValue>,
    // We could theoretically create sqlx::Query and bind it as soon as self.sql and self.args are final.
    // Unfortunately, this requires hitting the precise ProcRUSTean incantation required to have a Query field,
    // which, let's be honest, is never going to happen. >:-[
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
#[derive(Debug)]
struct IdTree {
    id: String,
    children: HashMap<String, IdTree>,
}

impl IdTree {
    fn to_json(&self) -> serde_json::Value {
        let mut ids_json = json!({"id": self.id});
        for (field_name, child_tree) in &self.children {
            ids_json[field_name.to_owned()] = child_tree.to_json();
        }
        ids_json
    }
}

/// Query engine.
///
/// The query engine provides a way to persist objects and retrieve them from
/// a backing store for ChiselStrike endpoints.
#[derive(Clone)]
pub(crate) struct QueryEngine {
    kind: Kind,
    pool: AnyPool,
}

impl QueryEngine {
    fn new(kind: Kind, pool: AnyPool) -> Self {
        Self { kind, pool }
    }

    pub(crate) async fn local_connection(conn: &DbConnection) -> anyhow::Result<Self> {
        let local = conn.local_connection().await?;
        Ok(Self::new(local.kind, local.pool))
    }

    pub(crate) async fn drop_table(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> anyhow::Result<()> {
        let drop_table = Table::drop()
            .table(Alias::new(ty.backing_table()))
            .to_owned();
        let drop_table = drop_table.build_any(DbConnection::get_query_builder(&self.kind));
        let drop_table = sqlx::query(&drop_table);

        transaction
            .execute(drop_table)
            .await
            .map_err(QueryError::ExecuteFailed)?;
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
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }

    pub(crate) async fn create_table(
        &self,
        transaction: &mut Transaction<'_, Any>,
        ty: &ObjectType,
    ) -> anyhow::Result<()> {
        let mut create_table = Table::create()
            .table(Alias::new(ty.backing_table()))
            .if_not_exists()
            .to_owned();

        for field in ty.all_fields() {
            let mut column_def = ColumnDef::try_from(field)?;
            create_table.col(&mut column_def);
        }
        let create_table = create_table.build_any(DbConnection::get_query_builder(&self.kind));

        let create_table = sqlx::query(&create_table);
        transaction
            .execute(create_table)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }

    pub(crate) async fn alter_table(
        &self,
        transaction: &mut Transaction<'_, Any>,
        old_ty: &ObjectType,
        delta: ObjectDelta,
    ) -> anyhow::Result<()> {
        // using a macro as async closures are unstable
        macro_rules! do_query {
            ( $table:expr ) => {{
                let table = $table.build_any(DbConnection::get_query_builder(&Kind::Postgres));
                let table = sqlx::query(&table);

                transaction
                    .execute(table)
                    .await
                    .map_err(QueryError::ExecuteFailed)
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
                .table(Alias::new(old_ty.backing_table()))
                .add_column(&mut column_def)
                .to_owned();

            do_query!(table)?;
        }

        for field in delta.removed_fields.iter() {
            let table = Table::alter()
                .table(Alias::new(old_ty.backing_table()))
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
        Ok(())
    }

    pub(crate) fn query_relation(&self, rel: Relation) -> SqlStream {
        sql(&self.pool, rel)
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
    pub(crate) async fn add_row(
        &self,
        ty: &ObjectType,
        ty_value: &JsonObject,
    ) -> anyhow::Result<serde_json::Value> {
        let (inserts, id_tree) = self.prepare_insertion(ty, ty_value)?;
        self.run_sql_queries(&inserts).await?;
        Ok(id_tree.to_json())
    }

    pub(crate) async fn add_row_shallow(
        &self,
        ty: &ObjectType,
        ty_value: &JsonObject,
    ) -> anyhow::Result<()> {
        let query = self.prepare_insertion_shallow(ty, ty_value)?;
        self.run_sql_queries(&[query]).await?;
        Ok(())
    }

    async fn run_sql_queries(&self, queries: &[SqlWithArguments]) -> anyhow::Result<()> {
        let mut transaction = self.start_transaction().await?;
        for q in queries {
            let mut sqlx_query = sqlx::query(&q.sql);
            for arg in &q.args {
                match arg {
                    SqlValue::Bool(arg) => sqlx_query = sqlx_query.bind(arg),
                    SqlValue::U64(arg) => sqlx_query = sqlx_query.bind(*arg as i64),
                    SqlValue::I64(arg) => sqlx_query = sqlx_query.bind(arg),
                    SqlValue::F64(arg) => sqlx_query = sqlx_query.bind(arg),
                    SqlValue::String(arg) => sqlx_query = sqlx_query.bind(arg),
                };
            }
            transaction
                .fetch_one(sqlx_query)
                .await
                .map_err(QueryError::ExecuteFailed)?;
        }
        QueryEngine::commit_transaction(transaction).await?;
        Ok(())
    }

    /// Recursively generates insert SQL queries necessary to insert object of type `ty`
    /// and value `ty_value` into database.
    /// Returns vector of SQL insert queries with corresponding arguments and IdTree of
    /// inserted objects.
    fn prepare_insertion(
        &self,
        ty: &ObjectType,
        ty_value: &JsonObject,
    ) -> anyhow::Result<(Vec<SqlWithArguments>, IdTree)> {
        let mut child_ids = HashMap::<String, IdTree>::new();
        let mut obj_id = Option::<String>::None;
        let mut query_args = Vec::<SqlValue>::new();
        let mut inserts = Vec::<SqlWithArguments>::new();

        for field in ty.all_fields() {
            let incompatible_data =
                || QueryError::IncompatibleData(field.name.to_owned(), ty.name().to_owned());
            let arg = match &field.type_ {
                Type::Object(nested_type) => {
                    if nested_type.name() == OAUTHUSER_TYPE_NAME {
                        anyhow::bail!("Cannot save into type {}.", OAUTHUSER_TYPE_NAME);
                    }
                    let nested_value = ty_value
                        .get(&field.name)
                        .context("json object doesn't have required field")
                        .with_context(incompatible_data)?
                        .as_object()
                        .context("unexpected json type (expected an object)")
                        .with_context(incompatible_data)?;

                    let nested_id = {
                        let (nested_inserts, nested_ids) =
                            self.prepare_insertion(nested_type, nested_value)?;
                        inserts.extend(nested_inserts);
                        let nested_id = nested_ids.id.to_owned();
                        child_ids.insert(field.name.to_owned(), nested_ids);
                        nested_id
                    };
                    SqlValue::String(nested_id)
                }
                _ => self
                    .convert_to_argument(field, ty_value)
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
            sql: self.make_insert_query(ty, ty_value)?,
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
    fn convert_to_argument(
        &self,
        field: &Field,
        ty_value: &JsonObject,
    ) -> anyhow::Result<SqlValue> {
        macro_rules! parse_default_value {
            (str, $value:expr) => {{
                $value
            }};
            ($fallback:ident, $value:expr) => {{
                let value: $fallback = $value
                    .as_str()
                    .parse()
                    .context("failed to parse default value")?;
                value
            }};
        }
        macro_rules! convert_json_value {
            ($as_type:ident, $fallback:ident) => {{
                match ty_value.get(&field.name) {
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

        let arg = match &field.type_ {
            Type::String | Type::Id | Type::Object(_) => {
                SqlValue::String(convert_json_value!(as_str, str))
            }
            Type::Float => SqlValue::F64(convert_json_value!(as_f64, f64)),
            Type::Boolean => SqlValue::Bool(convert_json_value!(as_bool, bool)),
        };
        Ok(arg)
    }

    /// For given object of type `ty` and its value `ty_value` computes a string
    /// representing SQL query which inserts the object into database.
    fn make_insert_query(&self, ty: &ObjectType, ty_value: &JsonObject) -> anyhow::Result<String> {
        let mut field_binds = String::new();
        let mut field_names = vec![];
        let mut id_name = String::new();
        let mut update_binds = String::new();
        let mut id_bind = String::new();

        for (i, f) in ty.all_fields().enumerate() {
            let bind = std::format!("${}", i + 1);
            field_binds.push_str(&bind);
            field_binds.push(',');

            field_names.push(f.name.clone());
            if f.type_ == Type::Id {
                if let Some(idstr) = ty_value.get(&f.name) {
                    let idstr = idstr
                        .as_str()
                        .ok_or_else(|| QueryError::InvalidId("not a string".into()))?;
                    Uuid::parse_str(idstr).map_err(|_| QueryError::InvalidId(idstr.into()))?;
                }
                anyhow::ensure!(id_bind.is_empty(), "More than one ID??");
                id_name = f.name.to_string();
                id_bind = bind.clone();
            }
            update_binds.push_str(&std::format!("{} = {},", &f.name, &bind));
        }
        field_binds.pop();
        update_binds.pop();

        for v in ty_value.keys() {
            anyhow::ensure!(
                field_names.contains(v),
                "field {} not present in {}",
                v,
                ty.name()
            );
        }

        Ok(std::format!(
            "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {} WHERE {} = {} RETURNING *",
            &ty.backing_table(),
            field_names.into_iter().join(","),
            field_binds,
            id_name,
            update_binds,
            id_name,
            id_bind,
        ))
    }

    fn prepare_insertion_shallow(
        &self,
        ty: &ObjectType,
        ty_value: &JsonObject,
    ) -> anyhow::Result<SqlWithArguments> {
        let mut query_args = Vec::<SqlValue>::new();
        for field in ty.all_fields() {
            let arg = self.convert_to_argument(field, ty_value).with_context(|| {
                QueryError::IncompatibleData(field.name.to_owned(), ty.name().to_owned())
            })?;
            query_args.push(arg);
        }

        Ok(SqlWithArguments {
            sql: self.make_insert_query(ty, ty_value)?,
            args: query_args,
        })
    }
}

pub(crate) fn relational_row_to_json(
    columns: &[(String, Type)],
    row: &AnyRow,
) -> anyhow::Result<JsonObject> {
    let mut ret = JsonObject::default();
    for (query_column, result_column) in zip(columns, row.columns()) {
        let i = result_column.ordinal();
        // FIXME: consider result_column.type_info().is_null() too
        macro_rules! to_json {
            ($value_type:ty) => {{
                let val = row.get::<$value_type, _>(i);
                json!(val)
            }};
        }
        let val = match query_column.1 {
            Type::Float => {
                // https://github.com/launchbadge/sqlx/issues/1596
                // sqlx gets confused if the float doesn't have decimal points.
                let val: &str = row.get_unchecked(i);
                json!(val.parse::<f64>()?)
            }
            Type::String => to_json!(&str),
            Type::Id => to_json!(&str),
            Type::Boolean => {
                // Similarly to the float issue, type information is not filled in
                // *if* this value was put in as a result of coalesce() (default).
                //
                // Also the database has integers ,and we need to map it back to a boolean
                // type on json.
                let val: &str = row.get_unchecked(i);
                let x: bool = val.parse::<usize>()? == 1;
                json!(x)
            }
            Type::Object(_) => anyhow::bail!("Relations aren't supported yet"),
        };
        ret.insert(result_column.name().to_string(), val);
    }
    Ok(ret)
}
