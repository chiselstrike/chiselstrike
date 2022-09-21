// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::http::{HttpRequest, HttpRequestResponse, HttpResponse};
use crate::kafka::KafkaEvent;
use crate::version::VersionJob;
use crate::worker::WorkerState;
use anyhow::{bail, Context, Result};
use guard::guard;
use serde::Serialize;
use std::cell::RefCell;
use std::rc::Rc;
use tokio::sync::oneshot;

/// A job that will be handled in JavaScript.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
enum AcceptedJob {
    #[serde(rename_all = "camelCase")]
    Http {
        request: HttpRequest,
        response_tx_rid: deno_core::ResourceId,
    },
    #[serde(rename_all = "camelCase")]
    Kafka { event: KafkaEvent },
}

/// A Deno resource that wraps a sender that is used to send response for an HTTP request.
///
/// This is passed to JavaScript along with the request (in `AcceptedJob::Http`), and JavaScript
/// then passes the response back to us by calling `op_chisel_http_respond`.
struct HttpResponseTxResource {
    response_tx: RefCell<Option<oneshot::Sender<HttpResponse>>>,
}

impl deno_core::Resource for HttpResponseTxResource {}

// Waits for the next job that should be handled by this worker. Returns `null` when there are no
// more jobs and the worker should terminate.
//
// This function must not be called concurrently (it will throw an exception otherwise).
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
            let response_tx_res = HttpResponseTxResource {
                response_tx: RefCell::new(Some(response_tx)),
            };
            let response_tx_rid = state.borrow_mut().resource_table.add(response_tx_res);
            AcceptedJob::Http {
                request,
                response_tx_rid,
            }
        }
        Some(VersionJob::Kafka(event)) => AcceptedJob::Kafka { event },
        None => return Ok(None),
    };

    Ok(Some(accepted_job))
}

#[deno_core::op]
fn op_chisel_http_respond(
    state: Rc<RefCell<deno_core::OpState>>,
    response_tx_rid: deno_core::ResourceId,
    response: HttpResponse,
) -> Result<()> {
    let response_tx = {
        let response_tx_res: Rc<HttpResponseTxResource> =
            state.borrow_mut().resource_table.take(response_tx_rid)?;
        let response_tx: &mut Option<_> = &mut response_tx_res.response_tx.borrow_mut();
        response_tx
            .take()
            .context("Response was already sent on this response sender")?
    };
    let _: Result<_, _> = response_tx.send(response);
    Ok(())
}
