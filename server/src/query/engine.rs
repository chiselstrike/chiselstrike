// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::db::{sql, Relation};
use crate::query::{DbConnection, Kind, QueryError};
use crate::rpc::chisel::field_definition::IdMarker;
use crate::types::{Field, ObjectDelta, ObjectType, Type};
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::StreamExt;
use itertools::zip;
use itertools::Itertools;
use sea_query::{Alias, ColumnDef, Table};
use serde_json::json;
use sqlx::any::{Any, AnyPool, AnyRow};
use sqlx::Column;
use sqlx::Transaction;
use sqlx::{Executor, Row};
use std::cell::RefCell;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};

// Results directly out of the database
pub(crate) type RawSqlStream = BoxStream<'static, anyhow::Result<AnyRow>>;

// Results with policies applied
pub(crate) type SqlStream = BoxStream<'static, anyhow::Result<serde_json::Value>>;

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
            Type::Int => column_def.integer(),
            Type::Float => column_def.double(),
            Type::Boolean => column_def.boolean(),
            Type::Object(_) => {
                anyhow::bail!(QueryError::NotImplemented(
                    "support for type Object".to_owned(),
                ));
            }
        };

        match field.id_marker {
            IdMarker::None => &mut column_def,
            IdMarker::Unique => column_def.unique_key(),
            IdMarker::Uuid => column_def.unique_key().primary_key().not_null(),
            IdMarker::AutoIncrement => column_def.auto_increment().primary_key().not_null(),
        };

        Ok(column_def)
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
            .map_err(QueryError::ConnectionFailed)?;
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

        let mut has_id = false;
        for field in &ty.fields {
            let mut column_def = ColumnDef::try_from(field)?;
            create_table.col(&mut column_def);
            has_id |= field.has_non_null_id();
        }

        if !has_id {
            create_table.col(
                ColumnDef::new(Alias::new("id"))
                    .integer()
                    .auto_increment()
                    .primary_key(),
            );
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
        let mut table = Table::alter()
            .table(Alias::new(old_ty.backing_table()))
            .to_owned();

        let mut modified = 0;
        for field in delta.added_fields.iter() {
            modified += 1;
            let mut column_def = ColumnDef::try_from(field)?;
            table.add_column(&mut column_def);
        }

        for field in delta.removed_fields.iter() {
            modified += 1;
            table.drop_column(Alias::new(&field.name));
        }

        // SQLite doesn't support modify columns at all, so we just ignore those, and will handle
        // any kind of modification on the application side. It also could be that we got
        // here, but didn't really modify anything at the table level
        if modified == 0 {
            return Ok(());
        }

        // alter table is problematic on SQLite (https://sqlite.org/lang_altertable.html)
        //
        // So we fake being Postgres. Our ALTERs should be well-behaved, but we then need to make
        // sure we're not doing any kind of operation that are listed among the problematic ones.
        //
        // In particular, we can't use defaults, which is fine since we can handle that on
        // chiselstrike's side.
        let table = table.build_any(DbConnection::get_query_builder(&Kind::Postgres));

        let table = sqlx::query(&table);
        transaction
            .execute(table)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }

    pub(crate) fn query_relation(&self, rel: &Relation) -> SqlStream {
        sql(&self.pool, rel)
    }

    pub(crate) async fn add_row(
        &self,
        ty: &ObjectType,
        ty_value: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let fields = ty.non_auto_increment_fields();
        let insert_query = std::format!(
            "INSERT INTO {} ({}) VALUES ({})",
            &ty.backing_table(),
            fields.iter().map(|f| &f.name).join(", "),
            (0..fields.len())
                .map(|i| std::format!("${}", i + 1))
                .join(", ")
        );

        let mut insert_query = sqlx::query(&insert_query);
        for field in fields.iter() {
            macro_rules! bind_default_json_value {
                (str, $value:expr) => {{
                    insert_query = insert_query.bind($value);
                }};
                ($fallback:ident, $value:expr) => {{
                    let value: $fallback = $value.as_str().parse().map_err(|_| {
                        QueryError::IncompatibleData(field.name.to_owned(), $value.clone())
                    })?;
                    insert_query = insert_query.bind(value);
                }};
            }

            macro_rules! bind_json_value {
                ($as_type:ident, $fallback:ident ) => {{
                    match ty_value.get(&field.name) {
                        Some(value_json) => {
                            let value = value_json.$as_type().ok_or_else(|| {
                                QueryError::IncompatibleData(
                                    field.name.to_owned(),
                                    ty.name().to_owned(),
                                )
                            })?;
                            insert_query = insert_query.bind(value);
                        }
                        None => {
                            let value = field.generate().ok_or_else(|| {
                                QueryError::IncompatibleData(
                                    field.name.to_owned(),
                                    ty.name().to_owned(),
                                )
                            })?;
                            bind_default_json_value!($fallback, value);
                        }
                    }
                }};
            }

            match field.type_ {
                Type::String => bind_json_value!(as_str, str),
                Type::Int => bind_json_value!(as_i64, i64),
                Type::Float => bind_json_value!(as_f64, f64),
                Type::Boolean => bind_json_value!(as_bool, bool),
                Type::Object(_) => {
                    anyhow::bail!(QueryError::NotImplemented(
                        "support for type Object".to_owned(),
                    ));
                }
            }
        }

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(QueryError::ConnectionFailed)?;
        transaction
            .execute(insert_query)
            .await
            .map_err(QueryError::ExecuteFailed)?;
        transaction
            .commit()
            .await
            .map_err(QueryError::ExecuteFailed)?;
        Ok(())
    }
}

pub(crate) fn relational_row_to_json(
    columns: &[(String, Type)],
    row: &AnyRow,
) -> anyhow::Result<serde_json::Value> {
    let mut ret = json!({});
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
            Type::Float => to_json!(f64),
            Type::Int => to_json!(i64),
            Type::String => to_json!(&str),
            Type::Boolean => to_json!(bool),
            Type::Object(_) => unreachable!("A column cannot be a Type::Object"),
        };
        ret[result_column.name()] = val;
    }
    Ok(ret)
}
