use anyhow::{Context, Result,};
use chisel_snapshot::schema;
use deno_core::v8;
use std::sync::Arc;
use crate::layout;
use crate::conn::DataConn;
use crate::query::{Query, InputParam, InputExpr, OutputExpr};
use crate::query::build::QueryBuilder;

pub fn find_by_id_query<'s>(
    conn: &DataConn,
    scope: &mut v8::HandleScope<'s>,
    entity_name: &schema::EntityName,
) -> Result<Query> {
    let table = get_entity_table(conn, entity_name)?;
    let entity = &conn.layout.schema.entities[entity_name];
    let mut q = QueryBuilder::new(conn.kind());

    let mut out_obj = Vec::new();
    q.sql.write("SELECT ");

    {
        q.sql.write(&table.id_col.col_name);
        let id_out_expr = OutputExpr::Id(table.id_col.repr, 0);
        out_obj.push((global_str(scope, "id"), id_out_expr));
    }

    for (i, field) in table.field_cols.values().enumerate() {
        q.sql.write(", ");
        q.sql.write(&field.col_name);
        let field_type = entity.fields[&field.field_name].type_.clone();
        let field_out_expr = OutputExpr::Field(field.repr, field_type, i + 1);
        out_obj.push((global_str(scope, &field.field_name), field_out_expr));
    }

    q.output(OutputExpr::Object(out_obj));

    q.sql.write(" FROM ");
    q.sql.write(&table.table_name);

    q.sql.write(" WHERE ");
    q.sql.write(&table.id_col.col_name);
    q.sql.write(" = ");

    let id_param = q.add_input(InputParam::Id(table.id_col.repr, InputExpr::Arg));
    q.sql.write_param(id_param);

    q.sql.write(" LIMIT 1");

    Ok(q.build(conn))
}

pub fn store_with_id_query<'s>(
    conn: &DataConn,
    scope: &mut v8::HandleScope<'s>,
    entity_name: &schema::EntityName,
) -> Result<Query> {
    let table = get_entity_table(conn, entity_name)?;
    let entity = &conn.layout.schema.entities[entity_name];
    let mut q = QueryBuilder::new(conn.kind());

    q.sql.write("INSERT INTO ");
    q.sql.write(&table.table_name);

    q.sql.write(" (");
    q.sql.write(&table.id_col.col_name);
    for field in table.field_cols.values() {
        q.sql.write(", ");
        q.sql.write(&field.col_name);
    }
    q.sql.write(") VALUES (");

    {
        let id_expr = InputExpr::Get(Box::new(InputExpr::Arg), global_str(scope, "id"));
        let id_param = q.add_input(InputParam::Id(table.id_col.repr, id_expr));
        q.sql.write_param(id_param);
    }

    for field in table.field_cols.values() {
        let field_expr = InputExpr::Get(Box::new(InputExpr::Arg), global_str(scope, &field.field_name));
        let field_type = entity.fields[&field.field_name].type_.clone();
        let field_param = q.add_input(InputParam::Field(field.repr, field_type, field_expr));
        q.sql.write_param(field_param);
    }

    q.sql.write(")");

    Ok(q.build(conn))
}

fn get_entity_table(conn: &DataConn, entity_name: &schema::EntityName) -> Result<Arc<layout::EntityTable>> {
    conn.layout.entity_tables.get(entity_name).cloned()
        .context("could not find entity in layout")
}

fn global_str<'s>(scope: &mut v8::HandleScope<'s>, value: &str) -> v8::Global<v8::String> {
    let local = v8::String::new(scope, value).unwrap();
    v8::Global::new(scope, local)
}
