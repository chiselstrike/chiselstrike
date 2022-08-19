// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::datastore::engine::SqlWithArguments;
use crate::datastore::query::SqlValue;
use crate::deno::lookup_builtin_type;
use crate::deno::query_engine_arc;
use crate::types::{Entity, Type};
use anyhow::Result;
use deno_core::OpState;
use sqlx::Row;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub const AUTH_USER_NAME: &str = "AuthUser";
pub const AUTH_SESSION_NAME: &str = "AuthSession";
pub const AUTH_TOKEN_NAME: &str = "AuthToken";
pub const AUTH_ACCOUNT_NAME: &str = "AuthAccount";

const AUTH_ENTITY_NAMES: [&str; 4] = [
    AUTH_USER_NAME,
    AUTH_SESSION_NAME,
    AUTH_TOKEN_NAME,
    AUTH_ACCOUNT_NAME,
];

pub fn is_auth_entity_name(entity_name: &str) -> bool {
    AUTH_ENTITY_NAMES.contains(&entity_name)
}

fn get_auth_user_type(state: &OpState) -> Result<Entity> {
    match lookup_builtin_type(state, AUTH_USER_NAME) {
        Ok(Type::Entity(t)) => Ok(t),
        _ => anyhow::bail!("Internal error: type AuthUser not found"),
    }
}

async fn add_crud_endpoint_for_type(
    type_name: &str,
    endpoint_name: &str,
    api: &mut ApiService,
) -> Result<()> {
    let mut sources = HashMap::new();
    sources.insert(
        format!("/__chiselstrike/endpoints/auth/{}", endpoint_name),
        format!(
            r#"
import {{ ChiselEntity }} from "@chiselstrike/api"
class {type_name} extends ChiselEntity {{}}
export default {type_name}.crud()"#
        ),
    );

    crate::server::add_endpoints(sources, api).await
}

pub async fn init(api: &mut ApiService) -> Result<()> {
    add_crud_endpoint_for_type(AUTH_USER_NAME, "users", api).await?;
    add_crud_endpoint_for_type(AUTH_SESSION_NAME, "sessions", api).await?;
    add_crud_endpoint_for_type(AUTH_TOKEN_NAME, "tokens", api).await?;
    add_crud_endpoint_for_type(AUTH_ACCOUNT_NAME, "accounts", api).await
}

/// Extracts the username of the logged-in user, or None if there was no login.
pub async fn get_username_from_id(
    state: Rc<RefCell<OpState>>,
    userid: Option<String>,
) -> Option<String> {
    let (qeng, user_type) = {
        let state = state.borrow();
        let qeng = query_engine_arc(&state);
        let user_type = get_auth_user_type(&state);
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
                        "SELECT email FROM \"{}\" WHERE id=$1", // For now, let's pretend email is username.
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
                Ok(row) => row.get("email"),
            }
        }
    }
}
