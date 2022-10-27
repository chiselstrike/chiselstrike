use super::WorkerState;
use anyhow::Result;
use deno_core::OpState;
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
