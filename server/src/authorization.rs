use http::request::Parts;
use sqlx::Row;

use crate::authentication::Authentication;
use crate::datastore::engine::SqlWithArguments;
use crate::datastore::query::SqlValue;
use crate::error::Result;
use crate::server::Server;
use crate::types::Entity;
use crate::types::Type;
use crate::version::Version;

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

fn get_auth_user_type(version: &Version) -> Result<Entity> {
    match version.type_system.lookup_builtin_type(AUTH_USER_NAME) {
        Ok(Type::Entity(t)) => Ok(t),
        _ => internal!("type AuthUser not found"),
    }
}

/// Extracts the username of the logged-in user, or None if there was no login.
async fn get_username_from_id(
    server: &Server,
    version: &Version,
    user_id: Option<&str>,
) -> Option<String> {
    let qeng = server.query_engine.clone();
    let user_type = get_auth_user_type(version);

    match (user_id, user_type) {
        (None, _) => None,
        (Some(_), Err(e)) => {
            log::warn!("{:?}", e);
            None
        }
        (Some(id), Ok(user_type)) => {
            match qeng
                .fetch_one(SqlWithArguments {
                    sql: format!(
                        "SELECT email FROM \"{}\" WHERE id=$1", // For now, let's pretend email is username.
                        user_type.backing_table()
                    ),
                    args: vec![SqlValue::String(id.into())],
                })
                .await
            {
                Err(_e) => {
                    //warn!("Username query error: {:?}", e);
                    None
                }
                Ok(row) => row.get("email"),
            }
        }
    }
}

async fn authorize_user_id(
    server: &Server,
    version: &Version,
    authentication: &Authentication,
    routing_path: &str,
) -> Result<()> {
    let user_id = authentication.user_id();
    let username = get_username_from_id(server, version, user_id).await;
    if !version
        .policy_system
        .user_authorization
        .is_allowed(username.as_deref(), routing_path)
    {
        forbidden!("Unauthorized user");
    }

    Ok(())
}

pub async fn authorize(
    server: &Server,
    version: &Version,
    authentication: &Authentication,
    routing_path: &str,
    req_parts: &Parts,
) -> Result<()> {
    authorize_user_id(server, version, authentication, routing_path).await?;

    let secrets = server.secrets.read();
    if !version
        .policy_system
        .secret_authorization
        .is_allowed(req_parts, &secrets, routing_path)
    {
        forbidden!("Invalid header authentication");
    }

    Ok(())
}
