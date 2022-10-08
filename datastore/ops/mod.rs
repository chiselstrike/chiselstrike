use anyhow::{Result, anyhow, bail};
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use crate::conn::DataConn;
use crate::ctx::DataCtx;

pub fn extension() -> deno_core::Extension {
    deno_core::ExtensionBuilder::default()
        .ops(vec![
            op_datastore_begin::decl(),
            op_datastore_commit::decl(),
            op_datastore_rollback::decl(),
            entity::op_datastore_find_by_id_query::decl(),
            entity::op_datastore_store_with_id_query::decl(),
            query::op_datastore_fetch_start::decl(),
            query::op_datastore_fetch::decl(),
            query::op_datastore_fetch_read::decl(),
            query::op_datastore_execute_start::decl(),
            query::op_datastore_execute::decl(),
            query::op_datastore_execute_rows_affected::decl(),
        ])
        .build()
}

#[deno_core::op]
async fn op_datastore_begin(
    op_state: Rc<RefCell<deno_core::OpState>>,
    conn_rid: deno_core::ResourceId,
) -> Result<deno_core::ResourceId> {
    let conn = op_state.borrow().resource_table.get::<DataConn>(conn_rid)?;
    let ctx = DataCtx::begin(&conn).await?;
    Ok(add_ctx(&mut op_state.borrow_mut(), ctx))
}

#[deno_core::op]
async fn op_datastore_commit(
    op_state: Rc<RefCell<deno_core::OpState>>,
    ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    with_consumed_ctx(&op_state, ctx_rid, |ctx| ctx.commit()).await
}

#[deno_core::op]
async fn op_datastore_rollback(
    op_state: Rc<RefCell<deno_core::OpState>>,
    ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    with_consumed_ctx(&op_state, ctx_rid, |ctx| ctx.rollback()).await
}


impl deno_core::Resource for DataConn {}


/// Deno resource that wraps a [`DataCtx`].
///
/// We need mutable access to the [`DataCtx`] to do operations, and we need to consume the
/// `DataCtx` to commit or rollback the transaction, so we wrap it in `RefCell<Option<_>>`.
struct DataCtxRes(RefCell<Option<DataCtx>>);
impl deno_core::Resource for DataCtxRes {}

fn add_ctx(op_state: &mut deno_core::OpState, ctx: DataCtx) -> deno_core::ResourceId {
    op_state.resource_table.add(DataCtxRes(RefCell::new(Some(ctx))))
}

macro_rules! with_borrowed_ctx {
    ($op_state:expr, $rid:expr, $ctx:ident => $body:block) => {{
        #[allow(unused_imports)]
        use ::anyhow::Context;
        let ctx_res_rc = $op_state.borrow().resource_table.get::<$crate::ops::DataCtxRes>($rid)?;
        let mut ctx_refmut = ctx_res_rc.0.try_borrow_mut()
            .context("could not borrow data context, another operation is in progress")?;
        match *ctx_refmut {
            Some(ref mut $ctx) => $body,
            None => ::anyhow::bail!("could not borrow data context that has been closed"),
        }
    }}
}

async fn with_consumed_ctx<F, Fut, T>(
    op_state: &RefCell<deno_core::OpState>,
    rid: deno_core::ResourceId,
    f: F,
) -> Result<T> 
    where F: FnOnce(DataCtx) -> Fut,
          Fut: Future<Output = Result<T>>,
{
    let ctx_res_rc: Rc<DataCtxRes> = op_state.borrow_mut().resource_table.take(rid)?;
    let ctx_res: DataCtxRes = Rc::try_unwrap(ctx_res_rc)
        .map_err(|_| anyhow!("could not consume data context, another operation is in progress"))?;
    let ctx: Option<DataCtx> = ctx_res.0.into_inner();
    match ctx {
        Some(ctx) => f(ctx).await,
        None => bail!("could not consume data context that has been closed"),
    }
}



// define the child modules at the end, so that they can use the macros
mod entity;
mod query;
