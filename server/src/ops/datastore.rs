// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

use anyhow::{anyhow, bail, Context as _, Result};
use deno_core::serde_v8::Serializable;
use deno_core::{serde_v8, v8, CancelFuture, OpState};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::WorkerState;
use crate::datastore::crud;
use crate::datastore::engine::{IdTree, QueryResults};
use crate::datastore::expr::Expr;
use crate::datastore::query::{Mutation, QueryOpChain, QueryPlan};
use crate::datastore::value::EntityValue;
use crate::ops::job_context::JobContext;
use crate::policy::{PolicyContext, PolicyProcessor};
use crate::types::{Entity, Type};
use crate::{feat_typescript_policies, JsonObject};

#[deno_core::op]
pub async fn op_chisel_begin_transaction(
    state: Rc<RefCell<OpState>>,
    job_ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    let query_engine = state
        .borrow()
        .borrow::<WorkerState>()
        .server
        .query_engine
        .clone();
    let data_ctx = {
        let state = state.borrow();
        let ctx = state.resource_table.get::<JobContext>(job_ctx_rid)?;
        let job_info = ctx.job_info.clone();
        let worker_state = state.borrow::<WorkerState>();
        let type_system = worker_state.version.type_system.clone();
        let policy_system = worker_state.version.policy_system.clone();
        let policy_engine = worker_state.policy_engine.clone();
        let policy_context = PolicyContext::new(policy_engine, ctx.job_info.clone());

        query_engine.create_data_context(type_system, policy_system, policy_context, job_info)
    }
    .await?;

    let ctx = state
        .borrow()
        .resource_table
        .get::<JobContext>(job_ctx_rid)?;
    let mut current_data_ctx = ctx.current_data_ctx.borrow_mut();

    anyhow::ensure!(
        current_data_ctx.is_none(),
        "A transaction is already open for this context"
    );

    current_data_ctx.replace(data_ctx);

    Ok(())
}

#[deno_core::op]
pub async fn op_chisel_commit_transaction(
    state: Rc<RefCell<OpState>>,
    job_ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    let ctx = state
        .borrow()
        .resource_table
        .get::<JobContext>(job_ctx_rid)?;
    let data_ctx = ctx
        .current_data_ctx
        .borrow_mut()
        .take()
        .context("Cannot commit a transaction because no transaction is in progress")?;

    data_ctx.commit().await?;

    Ok(())
}

#[deno_core::op]
pub fn op_chisel_rollback_transaction(
    state: &mut OpState,
    job_ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    let ctx = state.resource_table.get::<JobContext>(job_ctx_rid)?;
    ctx.current_data_ctx
        .borrow_mut()
        .take()
        .context("Cannot commit a transaction because no transaction is in progress")?
        .rollback()?;

    Ok(())
}

#[derive(Deserialize)]
pub struct StoreParams<'a> {
    name: String,
    value: serde_v8::Value<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    id_tree: IdTree,
    // TODO: return a v8 value instead, but require more work.
    value: JsonValue,
}

#[deno_core::op(v8)]
pub fn op_chisel_store<'a>(
    scope: &mut v8::HandleScope<'a>,
    state: Rc<RefCell<OpState>>,
    params: StoreParams<'a>,
    job_ctx_rid: deno_core::ResourceId,
) -> anyhow::Result<impl Future<Output = anyhow::Result<WriteResult>>> {
    let state = state.borrow();
    let v8_value = &params.value.v8_value;
    let value = EntityValue::from_v8(v8_value, scope)?;
    let worker_state = state.borrow::<WorkerState>();
    let server = worker_state.server.clone();
    let version_id = &worker_state.version.version_id;
    let ctx = state.resource_table.get::<JobContext>(job_ctx_rid)?;
    let ts = &worker_state.version.type_system;

    let ty = match ts.lookup_type(&params.name) {
        Ok(Type::Entity(ty)) => ty,
        _ => bail!("Cannot save into type {}", params.name),
    };
    if ty.is_auth() && !is_auth_path(version_id, ctx.job_info.path().unwrap_or("")) {
        bail!("Cannot save into auth type {}", params.name);
    }

    Ok(async move {
        let fut = {
            let data_ctx = ctx.data_context()?;
            server.query_engine.add_row(
                ty.object_type().clone(),
                value.try_into_map()?,
                &data_ctx,
            )?
        };

        let (id_tree, value) = fut.await?;

        Ok(WriteResult {
            id_tree,
            value: EntityValue::Map(value).into_json(),
        })
    })
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
    state: Rc<RefCell<OpState>>,
    params: DeleteParams,
    job_ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let (txn, mutation) = {
        let context = state
            .borrow()
            .resource_table
            .get::<JobContext>(job_ctx_rid)?;
        let data_ctx = context.data_context()?;
        let mutation =
            Mutation::delete_from_expr(&data_ctx, &params.type_name, &params.filter_expr).context(
                "failed to construct delete expression from JSON passed to `op_chisel_delete`",
            )?;
        (data_ctx.txn.clone(), mutation)
    };

    let mut txn = txn.lock().await;
    server
        .query_engine
        .mutate_with_transaction(mutation, &mut txn)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrudDeleteParams {
    type_name: String,
    url_query: Vec<(String, String)>,
}

#[deno_core::op]
pub async fn op_chisel_crud_delete(
    state: Rc<RefCell<OpState>>,
    params: CrudDeleteParams,
    job_ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let (txn, mutation) = {
        let context = state
            .borrow()
            .resource_table
            .get::<JobContext>(job_ctx_rid)?;
        let data_ctx = context.data_context()?;
        let mutation = crud::delete_from_url_query(&data_ctx, &params.type_name, &params.url_query)
            .context(
                "failed to construct delete expression from JSON passed to `op_chisel_crud_delete`",
            )?;
        (data_ctx.txn.clone(), mutation)
    };

    let mut txn = txn.lock().await;
    server
        .query_engine
        .mutate_with_transaction(mutation, &mut txn)
        .await?;
    Ok(())
}

#[deno_core::op]
pub async fn op_chisel_crud_query(
    state: Rc<RefCell<OpState>>,
    params: crud::QueryParams,
    job_ctx_rid: deno_core::ResourceId,
) -> Result<JsonObject> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    {
        let context = state
            .borrow()
            .resource_table
            .get::<JobContext>(job_ctx_rid)?;
        let context = context.current_data_ctx.borrow();
        let context = context.as_ref().context("No transaction in this context")?;
        server.query_engine.run_query(context, params)
    }
    .await
}

#[deno_core::op]
pub async fn op_chisel_relational_query_create(
    state: Rc<RefCell<OpState>>,
    op_chain: QueryOpChain,
    job_ctx_rid: deno_core::ResourceId,
) -> Result<deno_core::ResourceId> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let context = state
        .borrow()
        .resource_table
        .get::<JobContext>(job_ctx_rid)?;
    let data_ctx = context.data_context()?;
    let query_plan = QueryPlan::from_op_chain(&data_ctx, op_chain)?;
    let ty = query_plan.base_type().clone();

    let stream = server
        .query_engine
        .query(data_ctx.txn.clone(), query_plan)?;
    let resource = QueryStreamResource {
        stream: RefCell::new(stream),
        cancel: Default::default(),
        ty,
        next: RefCell::new(None),
    };
    let rid = state.as_ref().borrow_mut().resource_table.add(resource);

    Ok(rid)
}

type DbStream = RefCell<QueryResults>;

struct QueryStreamResource {
    stream: DbStream,
    cancel: deno_core::CancelHandle,
    ty: Entity,
    next: RefCell<Option<EntityValue>>,
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
    type Output = Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.resource.upgrade() {
            Some(rc) => {
                let mut stream = rc.stream.borrow_mut();
                let stream: &mut QueryResults = &mut stream;
                match stream.as_mut().poll_next(cx) {
                    Poll::Ready(Some(Ok(next))) => {
                        *rc.next.borrow_mut() = Some(EntityValue::Map(next));
                        Poll::Ready(Ok(()))
                    }
                    Poll::Ready(Some(Err(e))) => Poll::Ready(Err(e)),
                    Poll::Ready(None) => Poll::Ready(Ok(())),
                    Poll::Pending => Poll::Pending,
                }
            }
            None => Poll::Ready(Err(anyhow!("Closed resource"))),
        }
    }
}

#[deno_core::op]
pub async fn op_chisel_query_next(
    state: Rc<RefCell<OpState>>,
    query_stream_rid: deno_core::ResourceId,
    _job_ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    let (resource, cancel) = {
        let rc: Rc<QueryStreamResource> = state.borrow().resource_table.get(query_stream_rid)?;
        let cancel = deno_core::RcRef::map(&rc, |r| &r.cancel);
        (Rc::downgrade(&rc), cancel)
    };

    let fut = QueryNextFuture { resource };
    let fut = fut.or_cancel(cancel);
    fut.await?
}

#[deno_core::op(v8)]
pub fn op_chisel_query_get_value<'a>(
    scope: &mut v8::HandleScope<'a>,
    state: Rc<RefCell<OpState>>,
    query_stream_rid: deno_core::ResourceId,
    ctx: deno_core::ResourceId,
) -> Result<serde_v8::Value<'a>> {
    let query_stream: Rc<QueryStreamResource> =
        state.borrow().resource_table.get(query_stream_rid)?;
    let ty = query_stream.ty.object_type().clone();
    let v8_value = match query_stream.next.borrow_mut().take() {
        Some(v) => {
            if feat_typescript_policies() {
                let ctx = state
                    .borrow()
                    .resource_table
                    .get::<JobContext>(ctx)?
                    .data_context()?
                    .policy_context
                    .clone();
                let validator = PolicyProcessor { ty, ctx };
                validator
                    .process_read(v.try_into_map()?)?
                    .map(|mut v| v.to_v8(scope))
                    .transpose()?
                    .unwrap_or_else(|| v8::null(scope).into())
            } else {
                v.to_v8(scope)?
            }
        }
        None => v8::null(scope).into(),
    };
    Ok(serde_v8::Value::from(v8_value))
}
