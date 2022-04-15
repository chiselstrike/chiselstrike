// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiService, Body};
use crate::datastore::engine::SqlWithArguments;
use crate::datastore::query::SqlValue;
use crate::deno::get_meta;
use crate::deno::lookup_builtin_type;
use crate::deno::query_engine_arc;
use crate::types::{ObjectType, Type, OAUTHUSER_TYPE_NAME};
use crate::JsonObject;
use anyhow::Result;
use deno_core::OpState;
use http::Uri;
use hyper::{header, Request, Response, StatusCode};
use serde_json::json;
use sqlx::Row;
use std::cell::RefCell;
use std::rc::Rc;
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

fn get_oauth_user_type(state: &OpState) -> Result<Arc<ObjectType>> {
    match lookup_builtin_type(state, OAUTHUSER_TYPE_NAME) {
        Ok(Type::Object(t)) => Ok(t),
        _ => anyhow::bail!("Internal error: type {} not found", OAUTHUSER_TYPE_NAME),
    }
}

/// Upserts username into OAuthUser type, returning its ID.
async fn insert_user_into_db(state: Rc<RefCell<OpState>>, username: &str) -> Result<String> {
    let (oauth_user_type, query_engine) = {
        let state = state.borrow();
        let oauth_user_type = get_oauth_user_type(&state)?;
        let query_engine = query_engine_arc(&state);
        (oauth_user_type, query_engine)
    };
    let mut user = JsonObject::new();
    match query_engine
        .fetch_one(SqlWithArguments {
            sql: format!(
                "SELECT id FROM \"{}\" WHERE username=$1",
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
    Ok(query_engine
        .add_row(&oauth_user_type, &user, None)
        .await?
        .id)
}

pub(crate) async fn handle_callback(
    state: Rc<RefCell<OpState>>,
    url: Uri,
) -> Result<Response<Body>> {
    // TODO: Grab state out of the request, validate it, and grab the referrer URL out of it.
    let params = url.query();
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
    let meta = get_meta(&state.borrow());
    let userid = insert_user_into_db(state, &username).await?;

    Ok(redirect(&format!(
        // TODO: redirect to the URL from state.
        "http://localhost:3000/profile?chiselstrike_token={}",
        meta.new_session_token(&userid).await?
    )))
}

async fn add_crud_endpoint_for_type(
    type_name: &str,
    endpoint_name: &str,
    api: &mut ApiService,
) -> Result<()> {
    crate::server::add_endpoint(
        format!("/__chiselstrike/auth/{}", endpoint_name),
        format!(
            r#"
import {{ ChiselEntity }} from "@chiselstrike/api"
class {type_name} extends ChiselEntity {{}}
export default {type_name}.crud()"#
        ),
        api,
    )
    .await
}

pub(crate) async fn init(api: &mut ApiService) -> Result<()> {
    crate::server::add_endpoint(
        "/__chiselstrike/auth/callback",
        include_str!("auth_callback.js").to_string(),
        api,
    )
    .await?;
    crate::server::add_endpoint(
        "/__chiselstrike/auth/user/",
        include_str!("auth_user.js").to_string(),
        api,
    )
    .await?;

    add_crud_endpoint_for_type("NextAuthUser", "users", api).await?;
    add_crud_endpoint_for_type("NextAuthSession", "sessions", api).await?;
    add_crud_endpoint_for_type("NextAuthToken", "tokens", api).await?;
    add_crud_endpoint_for_type("NextAuthAccount", "accounts", api).await
}

/// Returns the user ID corresponding to the token in req.  If token is absent, returns None.
pub(crate) async fn get_user(
    state: Rc<RefCell<OpState>>,
    req: &Request<hyper::Body>,
) -> Result<Option<String>> {
    match req.headers().get("ChiselStrikeToken") {
        Some(token) => {
            let meta = get_meta(&state.borrow());
            Ok(meta.get_user_id(token.to_str()?).await.ok())
        }
        None => Ok(None),
    }
}

/// Extracts the username of the logged-in user, or None if there was no login.
pub(crate) async fn get_username_from_id(
    state: Rc<RefCell<OpState>>,
    userid: Option<String>,
) -> Option<String> {
    let (qeng, user_type) = {
        let state = state.borrow();
        let qeng = query_engine_arc(&state);
        let user_type = get_oauth_user_type(&state);
        (qeng, user_type)
    };
    match (userid, user_type) {
        (None, _) => None,
        (Some(_), Err(e)) => {
            warn!("{:?}", e);
            None
        }
        (Some(id), Ok(user_type)) => {
            match qeng
                .fetch_one(SqlWithArguments {
                    sql: format!(
                        "SELECT username FROM \"{}\" WHERE id=$1",
                        user_type.backing_table()
                    ),
                    args: vec![SqlValue::String(id)],
                })
                .await
            {
                Err(e) => {
                    warn!("Username query error: {:?}", e);
                    None
                }
                Ok(row) => row.get("username"),
            }
        }
    }
}
