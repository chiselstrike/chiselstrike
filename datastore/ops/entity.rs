use anyhow::Result;
use chisel_snapshot::schema;
use deno_core::v8;
use crate::conn::DataConn;
use crate::entity;

#[deno_core::op(v8)]
pub fn op_datastore_query_find_by_id<'s>(
    scope: &mut v8::HandleScope<'s>,
    op_state: &mut deno_core::OpState,
    conn_rid: deno_core::ResourceId,
    entity_name: schema::EntityName,
) -> Result<deno_core::ResourceId> {
    let conn = op_state.resource_table.get::<DataConn>(conn_rid)?;
    let query = entity::find_by_id_query(&conn, scope, &entity_name)?;
    let query_rid = op_state.resource_table.add(query);
    Ok(query_rid)
}

#[deno_core::op(v8)]
pub fn op_datastore_query_store_with_id<'s>(
    scope: &mut v8::HandleScope<'s>,
    op_state: &mut deno_core::OpState,
    conn_rid: deno_core::ResourceId,
    entity_name: schema::EntityName,
) -> Result<deno_core::ResourceId> {
    let conn = op_state.resource_table.get::<DataConn>(conn_rid)?;
    let query = entity::store_with_id_query(&conn, scope, &entity_name)?;
    let query_rid = op_state.resource_table.add(query);
    Ok(query_rid)
}
