// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
use std::fmt;

use anyhow::anyhow;
use sqlx::Row;

use crate::datastore::engine::SqlWithArguments;
use crate::datastore::query::SqlValue;
use crate::types::{Entity, Type};
use crate::{server::Server, version::Version};

type Result<T> = std::result::Result<T, AuthError>;

macro_rules! bad_request {
    ($($token:tt)*) => {
        return Err(AuthError::bad_request(anyhow!($($token)*)))
    };
}

macro_rules! forbidden {
    ($($token:tt)*) => {
        return Err(AuthError::forbidden(anyhow!($($token)*)))
    };
}

macro_rules! internal {
    ($($token:tt)*) => {
        return Err(AuthError::forbidden(anyhow!($($token)*)))
    };
}

#[derive(Debug)]
pub struct AuthError {
    pub inner: anyhow::Error,
    pub err_kind: AuthErrorKind,
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let context = match self.err_kind {
            AuthErrorKind::Forbbiden => "forbidden",
            AuthErrorKind::BadRequest => "bad request",
            AuthErrorKind::Internal => "internal error",
        };

        write!(f, "{context}: {}", self.inner)
    }
}

impl std::error::Error for AuthError {}

impl AuthError {
    fn internal(inner: anyhow::Error) -> Self {
        Self {
            inner,
            err_kind: AuthErrorKind::Internal,
        }
    }

    fn forbidden(inner: anyhow::Error) -> Self {
        Self {
            inner,
            err_kind: AuthErrorKind::Forbbiden,
        }
    }

    fn bad_request(inner: anyhow::Error) -> Self {
        Self {
            inner,
            err_kind: AuthErrorKind::BadRequest,
        }
    }
}

#[derive(Debug)]
pub enum AuthErrorKind {
    Forbbiden,
    BadRequest,
    Internal,
}

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
pub async fn get_username_from_id(
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
