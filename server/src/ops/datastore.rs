// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use super::request_context::RequestContext;
use super::WorkerState;
use crate::datastore::crud;
use crate::datastore::engine::{IdTree, QueryEngine, QueryResults, ResultRow};
use crate::datastore::expr::Expr;
use crate::datastore::query::{Mutation, QueryOpChain, QueryPlan};
use crate::types::Type;
use crate::JsonObject;
use anyhow::{anyhow, bail, Context as _, Result};
use deno_core::CancelFuture;
use serde_derive::Deserialize;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

#[deno_core::op]
pub async fn op_chisel_begin_transaction(
    state: Rc<RefCell<deno_core::OpState>>,
    ctx: deno_core::ResourceId,
) -> Result<()> {
    let query_engine = state
        .borrow()
        .borrow::<WorkerState>()
        .server
        .query_engine
        .clone();
    let ctx = state.borrow().resource_table.get::<RequestContext>(ctx)?;
    let transaction = query_engine.begin_transaction_static().await?;
    ctx.put_transaction(transaction);

    Ok(())
}

#[deno_core::op]
pub async fn op_chisel_commit_transaction(
    state: Rc<RefCell<deno_core::OpState>>,
    ctx: deno_core::ResourceId,
) -> Result<()> {
    let ctx = state.borrow().resource_table.get::<RequestContext>(ctx)?;
    let transaction = ctx
        .take_transaction()?
        .context("Cannot commit a transaction because no transaction is in progress")?;

    QueryEngine::commit_transaction(transaction).await?;

    Ok(())
}

#[deno_core::op]
pub fn op_chisel_rollback_transaction(
    state: &mut deno_core::OpState,
    ctx: deno_core::ResourceId,
) -> Result<()> {
    let ctx = state.resource_table.get::<RequestContext>(ctx)?;
    ctx.take_transaction()?
        .context("Cannot commit a transaction because no transaction is in progress")?;

    // Drop the transaction, causing it to rollback.
    Ok(())
}

#[derive(Deserialize)]
pub struct StoreParams {
    name: String,
    value: JsonObject,
}

#[deno_core::op]
pub async fn op_chisel_store(
    state: Rc<RefCell<deno_core::OpState>>,
    params: StoreParams,
    context: deno_core::ResourceId,
) -> Result<IdTree> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let context = state
        .borrow()
        .resource_table
        .get::<RequestContext>(context)?;
    let transaction = context.transaction()?;

    let ty = match context.type_system().lookup_type(&params.name) {
        Ok(Type::Entity(ty)) => ty,
        _ => bail!("Cannot save into type {}", params.name),
    };
    if ty.is_auth() && !is_auth_path(context.version_id(), context.request_path().unwrap_or("")) {
        bail!("Cannot save into auth type {}", params.name);
    }

    let mut transaction = transaction.lock().await;
    server
        .query_engine
        .add_row(
            &ty,
            &params.value,
            Some(&mut transaction),
            context.type_system(),
        )
        .await
}

fn is_auth_path(version_id: &str, routing_path: &str) -> bool {
    version_id == "__chiselstrike" && routing_path.starts_with("/auth/")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteParams {
    type_name: String,
    filter_expr: Option<Expr>,
}

#[deno_core::op]
pub async fn op_chisel_delete(
    state: Rc<RefCell<deno_core::OpState>>,
    params: DeleteParams,
    context: deno_core::ResourceId,
) -> Result<()> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let context = state
        .borrow()
        .resource_table
        .get::<RequestContext>(context)?;
    let mutation = Mutation::delete_from_expr(&*context, &params.type_name, &params.filter_expr)
        .context("failed to construct delete expression from JSON passed to `op_chisel_delete`")?;

    let transaction = context.transaction()?;
    let mut transaction = transaction.lock().await;
    server
        .query_engine
        .mutate_with_transaction(mutation, &mut transaction)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrudDeleteParams {
    type_name: String,
    url: String,
}

#[deno_core::op]
pub async fn op_chisel_crud_delete(
    state: Rc<RefCell<deno_core::OpState>>,
    params: CrudDeleteParams,
    context: deno_core::ResourceId,
) -> Result<()> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let context = state
        .borrow()
        .resource_table
        .get::<RequestContext>(context)?;
    let mutation = crud::delete_from_url(&*context, &params.type_name, &params.url).context(
        "failed to construct delete expression from JSON passed to `op_chisel_crud_delete`",
    )?;

    let transaction = context.transaction()?;
    let mut transaction = transaction.lock().await;
    server
        .query_engine
        .mutate_with_transaction(mutation, &mut transaction)
        .await?;
    Ok(())
}

#[deno_core::op]
pub async fn op_chisel_crud_query(
    state: Rc<RefCell<deno_core::OpState>>,
    params: crud::QueryParams,
    context: deno_core::ResourceId,
) -> Result<JsonObject> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let context = state
        .borrow()
        .resource_table
        .get::<RequestContext>(context)?;
    let transaction = context.transaction()?;
    crud::run_query(&*context, params, server.query_engine.clone(), transaction).await
}

#[deno_core::op]
pub async fn op_chisel_relational_query_create(
    state: Rc<RefCell<deno_core::OpState>>,
    op_chain: QueryOpChain,
    context: deno_core::ResourceId,
) -> Result<deno_core::ResourceId> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let context = state
        .borrow()
        .resource_table
        .get::<RequestContext>(context)?;
    let transaction = context.transaction()?;
    let query_plan = QueryPlan::from_op_chain(&*context, op_chain)?;

    let stream = server.query_engine.query(transaction, query_plan)?;
    let resource = QueryStreamResource {
        stream: RefCell::new(stream),
        cancel: Default::default(),
    };
    let rid = state.borrow_mut().resource_table.add(resource);

    Ok(rid)
}

type DbStream = RefCell<QueryResults>;

struct QueryStreamResource {
    stream: DbStream,
    cancel: deno_core::CancelHandle,
}

impl deno_core::Resource for QueryStreamResource {
    fn close(self: Rc<Self>) {
        self.cancel.cancel();
    }
}

// A future that resolves when this stream next element is available.
struct QueryNextFuture {
    resource: Weak<QueryStreamResource>,
}

impl Future for QueryNextFuture {
    type Output = Option<Result<ResultRow>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.resource.upgrade() {
            Some(rc) => {
                let mut stream = rc.stream.borrow_mut();
                let stream: &mut QueryResults = &mut stream;
                stream.as_mut().poll_next(cx)
            }
            None => Poll::Ready(Some(Err(anyhow!("Closed resource")))),
        }
    }
}

#[deno_core::op]
pub async fn op_chisel_query_next(
    state: Rc<RefCell<deno_core::OpState>>,
    query_stream_rid: deno_core::ResourceId,
) -> Result<Option<ResultRow>> {
    let (resource, cancel) = {
        let rc: Rc<QueryStreamResource> = state.borrow().resource_table.get(query_stream_rid)?;
        let cancel = deno_core::RcRef::map(&rc, |r| &r.cancel);
        (Rc::downgrade(&rc), cancel)
    };
    let fut = QueryNextFuture { resource };
    let fut = fut.or_cancel(cancel);
    if let Some(row) = fut.await? {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}
