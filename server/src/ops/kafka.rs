use super::WorkerState;
use anyhow::Result;
use deno_core::OpState;
use std::cell::RefCell;
use std::rc::Rc;

#[deno_core::op]
pub async fn op_chisel_subscribe_topic(state: Rc<RefCell<OpState>>, topic: String) -> Result<()> {
    let kafka_service = state
        .borrow()
        .borrow::<WorkerState>()
        .server
        .kafka_service
        .clone();
    if let Some(kafka_service) = kafka_service {
        kafka_service.subscribe_topic(&topic).await;
    }
    Ok(())
}
