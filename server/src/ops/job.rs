// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::http::{HttpRequest, HttpRequestResponse, HttpResponse};
use crate::kafka::KafkaEvent;
use crate::version::VersionJob;
use crate::worker::WorkerState;
use anyhow::{bail, Context, Result};
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
        response_rid: deno_core::ResourceId,
    },
    #[serde(rename_all = "camelCase")]
    Kafka { event: KafkaEvent },
}

struct HttpResponseResource {
    response_tx: RefCell<Option<oneshot::Sender<HttpResponse>>>,
}

impl deno_core::Resource for HttpResponseResource {}

#[deno_core::op]
#[allow(clippy::await_holding_refcell_ref)]
async fn op_chisel_accept_job(
    state: Rc<RefCell<deno_core::OpState>>,
) -> Result<Option<AcceptedJob>> {
    let job_rx: Rc<RefCell<_>> = state.borrow().borrow::<WorkerState>().job_rx.clone();
    let job = match job_rx.try_borrow_mut() {
        Ok(mut job_rx) => match job_rx.recv().await {
            Some(job) => job,
            None => return Ok(None),
        },
        Err(_) => bail!("op_chisel_accept_job cannot be called while another call is pending"),
    };

    let accepted_job = match job {
        VersionJob::Http(HttpRequestResponse {
            request,
            response_tx,
        }) => {
            let response_rid = {
                let response_res = HttpResponseResource {
                    response_tx: RefCell::new(Some(response_tx)),
                };
                state.borrow_mut().resource_table.add(response_res)
            };
            AcceptedJob::Http {
                request,
                response_rid,
            }
        }
        VersionJob::Kafka(event) => AcceptedJob::Kafka { event },
    };

    Ok(Some(accepted_job))
}

#[deno_core::op]
fn op_chisel_http_respond(
    state: Rc<RefCell<deno_core::OpState>>,
    response_rid: deno_core::ResourceId,
    response: HttpResponse,
) -> Result<()> {
    let response_tx = {
        let response_res: Rc<HttpResponseResource> =
            state.borrow_mut().resource_table.take(response_rid)?;
        let response_tx: &mut Option<_> = &mut response_res.response_tx.borrow_mut();
        response_tx
            .take()
            .context("Response was already sent on this response resource")?
    };
    let _: Result<_, _> = response_tx.send(response);
    Ok(())
}
