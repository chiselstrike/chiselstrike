// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::http::{HttpRequest, HttpRequestResponse, HttpResponse};
use crate::kafka::KafkaEvent;
use crate::ops::request_context::{RequestContext, RequestMeta};
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
                let inner = super::request_context::HttpRequest {
                    user_id,
                    path,
                    headers,
                    response_tx: RefCell::new(Some(response_tx)),
                };
                let ws = state.borrow::<WorkerState>();
                let ts = ws.version.type_system.clone();
                let ps = ws.version.policy_system.clone();
                let version_id = ws.version.version_id.clone();
                let ctx = RequestContext::new(ts, ps, version_id, inner.into());
                state.resource_table.add(ctx)
            };

            AcceptedJob::Http { request, ctx }
        }
        Some(VersionJob::Kafka(event)) => {
            let ctx = {
                let mut state = state.borrow_mut();
                let ws = state.borrow::<WorkerState>();
                let ts = ws.version.type_system.clone();
                let ps = ws.version.policy_system.clone();
                let version_id = ws.version.version_id.clone();
                let ctx = RequestContext::new(ts, ps, version_id, RequestMeta::KafkaEvent);
                state.resource_table.add(ctx)
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
    let ctx = state
        .borrow_mut()
        .resource_table
        .get::<RequestContext>(ctx)?;
    let ureq = ctx.http_request().context("invalid request type!")?;
    let tx = ureq
        .response_tx
        .borrow_mut()
        .take()
        .context("Response was already sent on this response tx resource")?;
    let _: Result<_, _> = tx.send(response);
    Ok(())
}
