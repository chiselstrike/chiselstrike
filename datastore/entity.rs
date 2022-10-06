use anyhow::{Context, Result, anyhow};
use chisel_snapshot::schema;
use sqlx::prelude::*;
use std::rc::Rc;
use crate::{layout, encode_v8, decode_v8};
use crate::context::DataCtx;

pub async fn find_entity_by_id<'s>(
    ctx: &mut DataCtx,
    scope: &mut v8::HandleScope<'s>,
    entity_name: &schema::EntityName,
    id_value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>> {
    let table = ctx.entity_table(entity_name)?;

    let stmt =
        if let Some(stmt) = ctx.find_by_id_cache.get(entity_name) {
            stmt.clone()
        } else {
            let sql = build_find_by_id_stmt(ctx, &table);
            let stmt = ctx.txn.prepare(&sql).await
                .context("could not prepare a statement to find entity by id")?;
            let stmt = Rc::new(stmt.to_owned());
            ctx.find_by_id_cache.insert(entity_name.clone(), stmt.clone());
            stmt
        };

    let mut args = sqlx::any::AnyArguments::default();
    encode_find_by_id_args(&table.id_col, scope, id_value, &mut args)?;
    let query = stmt.query_with(args);

    let row = match ctx.txn.fetch_one(query).await {
        Ok(row) => row,
        Err(sqlx::Error::RowNotFound) => return Ok(v8::undefined(scope).into()),
        Err(err) => return Err(anyhow!(err).context("SQL error when finding entity by id")),
    };

    let entity_obj = decode_find_by_id_row(&ctx.layout.schema, &table, scope, &row)
        .context("could not decode entity")?;
    Ok(entity_obj.into())
}

fn build_find_by_id_stmt(ctx: &DataCtx, table: &layout::EntityTable) -> String {
    let mut sql = ctx.sql_writer();
    sql.write("SELECT ");

    sql.write(&table.id_col.col_name);
    for field in table.field_cols.values() {
        sql.write(", ");
        sql.write(&field.col_name);
    }

    sql.write(" FROM ");
    sql.write(&table.table_name);

    sql.write(" WHERE ");
    sql.write(&table.id_col.col_name);
    sql.write(" = ");
    sql.write_param(0);

    sql.write(" LIMIT 1");

    sql.build()
}

fn encode_find_by_id_args<'s>(
    id_col: &layout::IdColumn,
    scope: &mut v8::HandleScope<'s>,
    id_value: v8::Local<'s, v8::Value>,
    out_args: &mut sqlx::any::AnyArguments,
) -> Result<()> {
    encode_v8::encode_id_to_sql(id_col, scope, id_value, out_args)
        .context("could not encode entity id from JS to SQL")
}

fn decode_find_by_id_row<'s>(
    schema: &schema::Schema,
    table: &layout::EntityTable,
    out_scope: &mut v8::HandleScope<'s>,
    row: &sqlx::any::AnyRow,
) -> Result<v8::Local<'s, v8::Object>> {
    let scope = &mut v8::EscapableHandleScope::new(out_scope);
    let entity_obj = v8::Object::new(scope);

    let id_value = decode_v8::decode_id_from_sql(&table.id_col, scope, row, 0)
        .context("could not decode id")?;
    let id_key = v8::String::new(scope, "id").unwrap();
    entity_obj.set(scope, id_key.into(), id_value).unwrap();

    for (i, field_col) in table.field_cols.values().enumerate() {
        let field_value = decode_v8::decode_field_from_sql(schema, table, field_col, scope, row, i + 1)
            .context("could not decode field")?;
        let field_key = v8::String::new(scope, &field_col.field_name).unwrap();
        entity_obj.set(scope, field_key.into(), field_value).unwrap();
    }

    Ok(scope.escape(entity_obj))
}

pub async fn store_entity_with_id<'s>(
    ctx: &mut DataCtx,
    scope: &mut v8::HandleScope<'s>,
    entity_name: &schema::EntityName,
    id_value: v8::Local<'s, v8::Value>,
    fields_obj: v8::Local<'s, v8::Object>,
) -> Result<()> {
    let table = ctx.entity_table(entity_name)?;

    let stmt =
        if let Some(stmt) = ctx.store_with_id_cache.get(entity_name) {
            stmt.clone()
        } else {
            let sql = build_store_with_id_stmt(ctx, &table);
            let stmt = ctx.txn.prepare(&sql).await
                .context("could not prepare a statement to store an entity with generated id")?;
            let stmt = Rc::new(stmt.to_owned());
            ctx.store_with_id_cache.insert(entity_name.clone(), stmt.clone());
            stmt
        };

    let mut args = sqlx::any::AnyArguments::default();
    encode_store_with_id_args(&ctx.layout.schema, &table, scope, id_value, fields_obj, &mut args)?;
    let query = stmt.query_with(args);

    ctx.txn.execute(query).await
        .context("SQL error when storing entity with id")?;
    Ok(())
}

fn build_store_with_id_stmt(ctx: &mut DataCtx, table: &layout::EntityTable) -> String {
    let mut sql = ctx.sql_writer();
    sql.write("INSERT INTO ");
    sql.write(&table.table_name);
    sql.write(" (");

    sql.write(&table.id_col.col_name);
    for field_col in table.field_cols.values() {
        sql.write(", ");
        sql.write(&field_col.col_name);
    }

    sql.write(") VALUES (");
    sql.write_param(0);
    for i in 0..table.field_cols.len() {
        sql.write(", ");
        sql.write_param(i + 1);
    }
    sql.write(")");

    sql.build()
}

fn encode_store_with_id_args<'s>(
    schema: &schema::Schema,
    table: &layout::EntityTable,
    scope: &mut v8::HandleScope<'s>,
    id_value: v8::Local<'s, v8::Value>,
    fields_obj: v8::Local<'s, v8::Object>,
    out_args: &mut sqlx::any::AnyArguments,
) -> Result<()> {
    encode_v8::encode_id_to_sql(&table.id_col, scope, id_value, out_args)
        .context("could not encode entity id from JS to SQL")?;
    for field_col in table.field_cols.values() {
        let field_key = v8::String::new(scope, &field_col.field_name).unwrap();
        let field_value = fields_obj.get(scope, field_key.into())
            .unwrap_or_else(|| v8::undefined(scope).into());
        encode_v8::encode_field_to_sql(schema, table, field_col, scope, field_value, out_args)
            .context("could not encode entity field from JS to SQL")?;
    }
    Ok(())
}
