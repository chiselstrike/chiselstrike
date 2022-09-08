// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::datastore::query::{
    RequestContext, RequestContextCell, RequestContextKind, UserRequest,
};
use crate::http::{HttpRequest, HttpRequestResponse, HttpResponse};
use crate::kafka::KafkaEvent;
use crate::version::VersionJob;
use crate::worker::WorkerState;
use anyhow::{bail, Context, Result};
use guard::guard;
use serde::Serialize;
use std::cell::RefCell;
use std::rc::Rc;

/// A job that will be handled in JavaScript.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
enum AcceptedJob {
    #[serde(rename_all = "camelCase")]
    Http {
        request: HttpRequest,
        ctx: deno_core::ResourceId,
    },
    #[serde(rename_all = "camelCase")]
    Kafka {
        event: KafkaEvent,
        ctx: deno_core::ResourceId,
    },
}

#[deno_core::op]
async fn op_chisel_accept_job(
    state: Rc<RefCell<deno_core::OpState>>,
) -> Result<Option<AcceptedJob>> {
    // temporarily move `job_rx` out of the `WorkerState`...
    guard! {let Some(mut job_rx) = state.borrow_mut().borrow_mut::<WorkerState>().job_rx.take() else {
        bail!("op_chisel_accept_job cannot be called while another call is pending")
    }};
    // ... wait for the job ...
    let received_job = job_rx.recv().await;
    // ... and move the `job_rx` back
    state.borrow_mut().borrow_mut::<WorkerState>().job_rx = Some(job_rx);

    let accepted_job = match received_job {
        Some(VersionJob::Http(request_response)) => {
            let HttpRequestResponse {
                request,
                response_tx,
            } = request_response;

            let ctx = {
                let mut state = state.borrow_mut();
                let user_id = request.user_id.clone();
                let path = request.routing_path.clone();
                let headers = request.headers.iter().cloned().collect();
                let inner = UserRequest {
                    user_id,
                    path,
                    headers,
                    response_tx: Some(response_tx),
                };
                let ctx = RequestContext::from_state(state.borrow(), inner.into());
                state.resource_table.add(RequestContextCell::new(ctx))
            };

            AcceptedJob::Http { request, ctx }
        }
        Some(VersionJob::Kafka(event)) => {
            let ctx = {
                let mut state = state.borrow_mut();
                let ctx =
                    RequestContext::from_state(state.borrow(), RequestContextKind::KafkaEvent);
                state.resource_table.add(RequestContextCell::new(ctx))
            };
            AcceptedJob::Kafka { event, ctx }
        }
        None => return Ok(None),
    };

    Ok(Some(accepted_job))
}

#[deno_core::op]
fn op_chisel_http_respond(
    state: Rc<RefCell<deno_core::OpState>>,
    ctx: deno_core::ResourceId,
    response: HttpResponse,
) -> Result<()> {
    let response_tx = {
        let ctx = state
            .borrow_mut()
            .resource_table
            .get::<RequestContextCell>(ctx)?;
        let mut ctx = ctx.borrow_mut();
        let ureq = ctx.user_request_mut().context("invalid request type!")?;
        ureq.response_tx
            .take()
            .context("Response was already sent on this response sender")?
    };
    let _: Result<_, _> = response_tx.send(response);
    Ok(())
}
