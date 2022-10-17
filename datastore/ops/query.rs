use anyhow::Result;
use deno_core::{v8, serde_v8};
use std::cell::RefCell;
use std::rc::Rc;
use crate::query::Query;
use crate::query::exec::{FetchStream, ExecuteFuture};

impl deno_core::Resource for Query {}



struct FetchStreamRes(RefCell<FetchStream>);
impl deno_core::Resource for FetchStreamRes {}

#[deno_core::op(v8)]
pub fn op_datastore_fetch_start<'s>(
    scope: &mut v8::HandleScope<'s>,
    op_state: &mut deno_core::OpState,
    query_rid: deno_core::ResourceId,
    arg: serde_v8::Value<'s>,
) -> Result<deno_core::ResourceId> {
    let query = op_state.resource_table.get::<Query>(query_rid)?;
    let stream = FetchStream::start(query, scope, arg.into())?;
    let stream_res = FetchStreamRes(RefCell::new(stream));
    Ok(op_state.resource_table.add(stream_res))
}

#[deno_core::op]
pub async fn op_datastore_fetch(
    op_state: Rc<RefCell<deno_core::OpState>>,
    ctx_rid: deno_core::ResourceId,
    stream_rid: deno_core::ResourceId,
) -> Result<bool> {
    with_borrowed_ctx!(op_state, ctx_rid, ctx => {
        let stream_res = op_state.borrow().resource_table.get::<FetchStreamRes>(stream_rid)?;
        let mut stream = stream_res.0.borrow_mut();
        stream.fetch(ctx).await
    })
}

#[deno_core::op(v8)]
pub fn op_datastore_fetch_read<'s>(
    scope: &mut v8::HandleScope<'s>,
    op_state: &mut deno_core::OpState,
    stream_rid: deno_core::ResourceId,
) -> Result<serde_v8::Value<'s>> {
    let stream_res = op_state.resource_table.get::<FetchStreamRes>(stream_rid)?;
    let stream = &stream_res.0.borrow_mut();
    let value = stream.read(scope)?;
    Ok(value.into())
}



struct ExecuteFutureRes(RefCell<ExecuteFuture>);
impl deno_core::Resource for ExecuteFutureRes {}

#[deno_core::op(v8)]
pub fn op_datastore_execute_start<'s>(
    scope: &mut v8::HandleScope<'s>,
    op_state: &mut deno_core::OpState,
    query_rid: deno_core::ResourceId,
    arg: serde_v8::Value<'s>,
) -> Result<deno_core::ResourceId> {
    let query = op_state.resource_table.get::<Query>(query_rid)?;
    let future = ExecuteFuture::start(query, scope, arg.into())?;
    let future_res = ExecuteFutureRes(RefCell::new(future));
    Ok(op_state.resource_table.add(future_res))
}

#[deno_core::op]
pub async fn op_datastore_execute(
    op_state: Rc<RefCell<deno_core::OpState>>,
    ctx_rid: deno_core::ResourceId,
    future_rid: deno_core::ResourceId,
) -> Result<()> {
    with_borrowed_ctx!(op_state, ctx_rid, ctx => {
        let future_res = op_state.borrow().resource_table.get::<ExecuteFutureRes>(future_rid)?;
        let mut future = future_res.0.borrow_mut();
        future.execute(ctx).await
    })
}

#[deno_core::op]
pub fn op_datastore_execute_rows_affected(
    op_state: &mut deno_core::OpState,
    future_rid: deno_core::ResourceId,
) -> Result<u64> {
    let future_res = op_state.resource_table.get::<ExecuteFutureRes>(future_rid)?;
    let future = &future_res.0.borrow_mut();
    future.rows_affected()
}

