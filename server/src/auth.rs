// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiService, Body};
use crate::query::engine::{JsonObject, SqlWithArguments};
use crate::runtime;
use crate::types::{ObjectType, Type, OAUTHUSER_TYPE_NAME};
use anyhow::anyhow;
use futures::{Future, FutureExt};
use hyper::{header, Request, Response, StatusCode};
use serde_json::json;
use sqlx::Row;
use std::pin::Pin;
use std::sync::Arc;

fn redirect(link: &str) -> Response<Body> {
    let bd: Body = format!("Redirecting to <a href='{}'>{}</a>\n", link, link).into();
    Response::builder()
        .status(StatusCode::TEMPORARY_REDIRECT)
        .header(header::LOCATION, link)
        .body(bd)
        .unwrap()
}

fn bad_request(msg: String) -> Response<Body> {
    let bd: Body = (msg + "\n").into();
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(bd)
        .unwrap()
}

pub(crate) fn get_oauth_user_type() -> anyhow::Result<Arc<ObjectType>> {
    match runtime::get()
        .type_system
        .lookup_builtin_type(OAUTHUSER_TYPE_NAME)
    {
        Ok(Type::Object(t)) => Ok(t),
        _ => anyhow::bail!("Internal error: type {} not found", OAUTHUSER_TYPE_NAME),
    }
}

/// Upserts username into OAuthUser type, returning its ID.
async fn insert_user_into_db(username: &str) -> anyhow::Result<String> {
    let oauth_user_type = get_oauth_user_type()?;
    let mut user = JsonObject::new();
    let query_engine = { runtime::get().query_engine.clone() };
    match query_engine
        .fetch_one(SqlWithArguments {
            sql: format!(
                "SELECT id FROM {} WHERE username=$1",
                oauth_user_type.backing_table()
            ),
            args: vec![username.into()],
        })
        .await
    {
        Err(_) => { /* Presume the ID just isn't in the database because this is a new user. */ }
        Ok(row) => {
            user.insert("id".into(), serde_json::Value::String(row.get("id")));
        }
    }
    user.insert("username".into(), json!(username));
    query_engine
        .add_row(&oauth_user_type, &user)
        .await?
        .get("id")
        .ok_or_else(|| anyhow!("Didn't get user ID from storing a user."))?
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("User ID wasn't a string."))
}

fn handle_callback(
    req: Request<hyper::Body>,
) -> Pin<Box<dyn Future<Output = Result<Response<Body>, anyhow::Error>>>> {
    // TODO: Grab state out of the request, validate it, and grab the referrer URL out of it.
    async move {
        let params = req.uri().query();
        if params.is_none() {
            return Ok(bad_request("Callback error: parameter missing".into()));
        }
        let username = params.unwrap().strip_prefix("user=");
        if username.is_none() {
            return Ok(bad_request(
                "Callback error: parameter value missing".into(),
            ));
        }
        let username = username.unwrap();
        if username.is_empty() {
            return Ok(bad_request("Callback error: parameter value empty".into()));
        }
        let username = urldecode::decode(username.into());
        let userid = insert_user_into_db(&username).await?;
        let meta = runtime::get().meta.clone();
        Ok(redirect(&format!(
            // TODO: redirect to the URL from state.
            "http://localhost:3000/profile?chiselstrike_token={}",
            meta.new_session_token(&userid).await?
        )))
    }
    .boxed_local()
}

pub(crate) fn init(api: &mut ApiService) {
    api.add_route(
        "/__chiselstrike/auth/callback".into(),
        Arc::new(handle_callback),
    );
}

/// Returns the user ID corresponding to the token in req.  If token is absent, returns None.
pub(crate) async fn get_user(req: &Request<hyper::Body>) -> anyhow::Result<Option<String>> {
    match req.headers().get("ChiselStrikeToken") {
        Some(token) => {
            let meta = { crate::runtime::get().meta.clone() };
            Ok(meta.get_user_id(token.to_str()?).await.ok())
        }
        None => Ok(None),
    }
}
