use super::WorkerState;
use crate::datastore::expr::{BinaryExpr, Expr, PropertyAccess, Value};
use crate::datastore::query::{Mutation, QueryOp, SortBy, SortKey};
use crate::datastore::QueryEngine;
use crate::datastore::{
    query::QueryPlan,
    value::{EntityMap, EntityValue},
};
use crate::ops::job_context::JobContext;
use crate::outbox::OUTBOX_NAME;
use crate::policy::PolicyContext;
use crate::types::Type;
use anyhow::Result;
use deno_core::OpState;
use futures::StreamExt;
use std::cell::RefCell;
use std::rc::Rc;

#[deno_core::op]
pub fn op_chisel_subscribe_topic(op_state: Rc<RefCell<OpState>>, topic: String) -> Result<()> {
    let server = op_state.borrow().borrow::<WorkerState>().server.clone();
    if let Some(ref service) = server.kafka_service {
        service.subscribe_topic(server.clone(), topic);
    }
    Ok(())
}

#[deno_core::op]
pub async fn op_chisel_publish(op_state: Rc<RefCell<OpState>>) -> Result<()> {
    let server = op_state.borrow().borrow::<WorkerState>().server.clone();
    if let Some(ref service) = server.kafka_service {
        service.publish(server.clone()).await?;
    }
    Ok(())
}

#[deno_core::op]
pub async fn op_chisel_poll_outbox(
    state: Rc<RefCell<OpState>>,
    job_ctx_rid: deno_core::ResourceId,
) -> Result<()> {
    let server = state.borrow().borrow::<WorkerState>().server.clone();
    let kafka_service = match &server.kafka_service {
        Some(kafka_service) => kafka_service.clone(),
        _ => {
            return Ok(());
        }
    };
    let _poll_mutex = kafka_service.outbox_poll_mutex.lock().await;
    let query_engine = server.query_engine.clone();
    let (data_ctx_future, outbox_type) = {
        let state = state.borrow();
        let ctx = state.resource_table.get::<JobContext>(job_ctx_rid)?;
        let job_info = ctx.job_info.clone();
        let worker_state = state.borrow::<WorkerState>();
        let type_system = worker_state.version.type_system.clone();
        let policy_system = worker_state.version.policy_system.clone();
        let policy_engine = worker_state.policy_engine.clone();
        let policy_context = PolicyContext::new(policy_engine, ctx.job_info.clone());
        let outbox_type = match type_system.lookup_builtin_type(OUTBOX_NAME)? {
            Type::Entity(entity) => entity,
            _ => anyhow::bail!("internal error"),
        };
        let data_ctx_future =
            query_engine.create_data_context(type_system, policy_system, policy_context, job_info);
        (data_ctx_future, outbox_type)
    };
    let data_ctx = data_ctx_future.await?;
    let ops = vec![QueryOp::SortBy(SortBy {
        keys: vec![SortKey {
            field_name: "seqNo".to_string(),
            ascending: true,
        }],
    })];
    let query_plan = QueryPlan::from_ops(&data_ctx, &outbox_type, ops)?;
    // NOTE! We collect the query results here because if we have the
    // QueryResults object alive while calling mutate_with_transaction(),
    // we deadlock.
    let rows: Vec<Result<EntityMap>> = query_engine
        .query(data_ctx.txn.clone(), query_plan)?
        .collect()
        .await;
    for row in rows {
        let row = row?;
        let topic = match row.get("topic") {
            Some(EntityValue::String(val)) => val,
            _ => anyhow::bail!("internal error"),
        };
        let key = match row.get("key") {
            Some(EntityValue::Bytes(val)) => Some(val.to_vec()),
            _ => None,
        };
        let value = match row.get("value") {
            Some(EntityValue::Bytes(val)) => Some(val.to_vec()),
            _ => None,
        };
        kafka_service.publish_event(topic, key, value).await?;
        let left = Expr::from(PropertyAccess {
            object: Box::new(Expr::Parameter { position: 0 }),
            property: "id".to_string(),
        });
        let id = match row.get("id") {
            Some(EntityValue::String(val)) => val,
            _ => anyhow::bail!("internal error"),
        };
        let right = Expr::from(Value::from(id.to_string()));
        let expr = BinaryExpr::eq(left, right);
        let mutation = Mutation::delete_from_expr(&data_ctx, OUTBOX_NAME, &Some(expr))?;
        let mut delete_txn = query_engine.begin_transaction().await?;
        query_engine
            .mutate_with_transaction(mutation, &mut delete_txn)
            .await?;
        QueryEngine::commit_transaction(delete_txn).await?;
    }
    QueryEngine::commit_transaction_static(data_ctx.txn).await?;
    Ok(())
}
