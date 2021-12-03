// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiService, Body};
use crate::runtime;
use anyhow::anyhow;
use futures::{Future, FutureExt};
use hyper::{header, Request, Response, StatusCode};
use std::pin::Pin;

const USERPATH: &str = "/__chiselstrike/auth/user/";

fn redirect(link: &str) -> Response<Body> {
    let bd: Body = format!("Redirecting to <a href='{}'>{}</a>\n", link, link).into();
    Response::builder()
        .status(StatusCode::TEMPORARY_REDIRECT)
        .header(header::LOCATION, link)
        .body(bd)
        .unwrap()
}

fn handle_callback(
    req: Request<hyper::Body>,
) -> Pin<Box<dyn Future<Output = Result<Response<Body>, anyhow::Error>>>> {
    // TODO: Grab state out of the request, validate it, and grab the referrer URL out of it.
    async move {
        let params = req.uri().query();
        if params.is_none() {
            return Err(anyhow!("Callback error: parameter missing"));
        }
        let username = params.unwrap().strip_prefix("user=");
        if username.is_none() {
            return Err(anyhow!("Callback error: parameter value missing"));
        }
        let username = username.unwrap();
        if username.is_empty() {
            return Err(anyhow!("Callback error: parameter value empty"));
        }
        Ok(redirect(&format!(
            // TODO: redirect to the URL from state.
            "http://localhost:3000/profile?chiselstrike_token={}",
            runtime::get()
                .await
                .meta
                .new_session_token(&urldecode::decode(username.into()))
                .await?
        )))
    }
    .boxed_local()
}

fn lookup_user(
    req: Request<hyper::Body>,
) -> Pin<Box<dyn Future<Output = Result<Response<Body>, anyhow::Error>>>> {
    async move {
        let token = req
            .uri()
            .path()
            .strip_prefix(USERPATH)
            .ok_or_else(|| anyhow!("lookup_user on wrong URL"))?;
        let bd: Body = runtime::get().await.meta.get_username(token).await?.into();
        let resp = Response::builder().status(StatusCode::OK).body(bd).unwrap();
        Ok(resp)
    }
    .boxed_local()
}

pub(crate) fn init(api: &mut ApiService) {
    api.add_route("/__chiselstrike/auth/callback", Box::new(handle_callback));
    api.add_route(USERPATH, Box::new(lookup_user));
}
