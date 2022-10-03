// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{bail, Context, Result};
use guard::guard;
use serde::Serialize;

use crate::http::{HttpRequest, HttpRequestResponse, HttpResponse};
use crate::kafka::KafkaEvent;
use crate::ops::job_context::{JobContext, JobInfo};
use crate::version::VersionJob;
use crate::worker::WorkerState;

/// A job that will be handled in JavaScript.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
enum AcceptedJob {
    #[serde(rename_all = "camelCase")]
    Http {
        request: HttpRequest,
        ctx_rid: deno_core::ResourceId,
    },
    #[serde(rename_all = "camelCase")]
    Kafka {
        event: KafkaEvent,
        ctx_rid: deno_core::ResourceId,
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
    let mut state = state.borrow_mut();
    state.borrow_mut::<WorkerState>().job_rx = Some(job_rx);

    let accepted_job = match received_job {
        Some(VersionJob::Http(request_response)) => {
            let HttpRequestResponse {
                request,
                response_tx,
            } = request_response;

            let ctx_rid = {
                let path = request.routing_path.clone();
                let headers = request.headers.iter().cloned().collect();
                let method = request.method.clone();
                let user_id = request.user_id.clone();
                let response_tx = RefCell::new(Some(response_tx));

                let job_info = Rc::new(JobInfo::HttpRequest {
                    method,
                    path,
                    headers,
                    user_id,
                    response_tx,
                });

                let ctx = JobContext {
                    current_data_ctx: None.into(),
                    job_info,
                };

                state.resource_table.add(ctx)
            };

            AcceptedJob::Http { request, ctx_rid }
        }
        Some(VersionJob::Kafka(event)) => {
            let ctx_rid = {
                let ctx = JobContext {
                    job_info: Rc::new(JobInfo::KafkaEvent),
                    current_data_ctx: None.into(),
                };
                state.resource_table.add(ctx)
            };
            AcceptedJob::Kafka { event, ctx_rid }
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
    let ctx = state.borrow_mut().resource_table.get::<JobContext>(ctx)?;
    match *ctx.job_info {
        JobInfo::HttpRequest {
            ref response_tx, ..
        } => {
            let tx = response_tx
                .borrow_mut()
                .take()
                .context("Response already send for that request")?;
            let _ = tx.send(response);
        }
        _ => bail!("invalid request type"),
    }

    Ok(())
}
