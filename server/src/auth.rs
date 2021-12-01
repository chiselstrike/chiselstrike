// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiService, Body};
use futures::Future;
use hyper::{header, Request, Response, StatusCode};
use std::pin::Pin;

fn redirect(link: &str) -> Response<Body> {
    let bd: Body = format!("Redirecting to <a href='{}'>{}</a>\n", link, link).into();
    Response::builder()
        .status(StatusCode::TEMPORARY_REDIRECT)
        .header(header::LOCATION, link)
        .body(bd)
        .unwrap()
}

fn handle_callback(
    _req: Request<hyper::Body>,
) -> Pin<Box<dyn Future<Output = Result<Response<Body>, anyhow::Error>>>> {
    // TODO: Grab state out of the request, validate it, and grab the referrer URL out of it.
    let token = "12345"; // TODO: Generate a unique token for each session.
    use futures::FutureExt;
    // TODO: redirect to the URL from state.
    std::future::ready(Ok(redirect(&format!(
        "http://localhost:3000/profile?chiselstrike_token={}",
        token
    ))))
    .boxed_local()
}

pub(crate) fn init(api: &mut ApiService) {
    api.add_route("/__chiselstrike/auth/callback", Box::new(handle_callback));
}
