// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiRequest, ApiResponse, ApiRequestResponse};
use crate::worker::WorkerState;
use anyhow::{Context, Result};
use std::cell::RefCell;
use std::rc::Rc;
use tokio::sync::oneshot;


struct ResponseResource {
    response_tx: RefCell<Option<oneshot::Sender<ApiResponse>>>,
}

impl deno_core::Resource for ResponseResource { }

#[deno_core::op]
async fn op_chisel_accept(state: Rc<RefCell<deno_core::OpState>>)
    -> Option<(ApiRequest, deno_core::ResourceId)>
{
    let request_rx = state.borrow().borrow::<WorkerState>().request_rx.clone();
    let request_response = request_rx.recv().await.ok()?;
    let ApiRequestResponse { request, response_tx } = request_response;

    let response_rid = {
        let response_res = ResponseResource {
            response_tx: RefCell::new(Some(response_tx)),
        };
        state.borrow_mut().resource_table.add(response_res)
    };
    Some((request, response_rid))
}

#[deno_core::op]
fn op_chisel_respond(
    state: Rc<RefCell<deno_core::OpState>>,
    response_rid: deno_core::ResourceId,
    response: ApiResponse,
) -> Result<()> {
    let response_tx = {
        let response_res: Rc<ResponseResource> = state.borrow_mut().resource_table.take(response_rid)?;
        let response_tx: &mut Option<_> = &mut response_res.response_tx.borrow_mut();
        response_tx.take()
            .context("Response was already sent on this response resource")?
    };
    let _: Result<_, _> = response_tx.send(response);
    Ok(())
}

