// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
use std::fmt;

use anyhow::anyhow;
use http::request::Parts;
use jsonwebtoken::{DecodingKey, Validation};
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::Value as JsonValue;
use sqlx::Row;

use crate::datastore::engine::SqlWithArguments;
use crate::datastore::query::SqlValue;
use crate::types::{Entity, Type};
use crate::JsonObject;
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

/// Represents a Authenticated user
#[derive(Debug, Serialize, Clone)]
pub enum Authentication {
    /// Claims from a user authenticated through JWT, passed in the `Authorization: Bearer` header.
    Jwt(JsonValue),
    /// User id of the authencated user, if he's logged with a user id passed in the header
    /// ChiselUID
    UserId(String),
    /// No authenticated user
    None,
}

async fn authenticate_by_user_id(
    server: &Server,
    version: &Version,
    user_id: Option<String>,
    routing_path: &str,
) -> Result<Authentication> {
    let username = get_username_from_id(server, version, user_id.as_deref()).await;
    if !version
        .policy_system
        .user_authorization
        .is_allowed(username.as_deref(), routing_path)
    {
        forbidden!("Unauthorized user");
    }

    Ok(user_id
        .map(Authentication::UserId)
        .unwrap_or(Authentication::None))
}

/// Takes a string representing a RSA PEM with its header and footer trimmed, and all line breaks
/// removed, and created a decoding key for decoding a JWT.
fn decoding_key_from_pem(key: &str) -> Result<DecodingKey> {
    let header = String::from("-----BEGIN PUBLIC KEY-----\n");
    let mut key = key
        .as_bytes()
        .chunks(64)
        .try_fold(header, |mut buf, s| -> Result<String> {
            let line = std::str::from_utf8(s)
                .map_err(|_| AuthError::internal(anyhow!("JWT signing key is not valid UTF-8")))?;
            buf.push_str(line);
            buf.push('\n');
            Ok(buf)
        })?;
    key.push_str("-----END PUBLIC KEY-----");

    let dkey = DecodingKey::from_rsa_pem(key.as_bytes()).map_err(|_| {
        AuthError::internal(anyhow!("JWT signing key is not a valid RSA PEM string"))
    })?;

    Ok(dkey)
}

/// Attempts to authenticate the provided JWT.
/// First looks for the `CHISEL_JWT_VALIDATION_KEY` in the instance secrets, in a RSA pem format,
/// with the header and footer trimmed, and all line breaks removed.
///
/// Once the key is retrieved, the token is validated with the RS256 algorithm, and the claims are
/// returned in the Authentication::Jwt enum
fn authenticate_jwt(secrets: &RwLock<JsonObject>, token: &str) -> Result<Authentication> {
    // get public signing key.
    let lock = secrets.read();
    let secret_value = lock.get("CHISEL_JWT_VALIDATION_KEY").unwrap();
    match secret_value.as_str() {
        Some(pem_str) => {
            let dkey = decoding_key_from_pem(pem_str)?;
            let json = validate_token(token, dkey)?;
            Ok(Authentication::Jwt(json))
        }
        None => internal!(r#""CHISEL_JWT_VALIDATION_KEY" should be a valid UTF-8 string."#),
    }
}

fn validate_token(token: &str, key: DecodingKey) -> Result<JsonValue> {
    match jsonwebtoken::decode::<JsonValue>(
        token,
        &key,
        &Validation::new(jsonwebtoken::Algorithm::RS256),
    ) {
        Ok(token) => Ok(token.claims),
        Err(e) => forbidden!("invalid token: {e}"),
    }
}

async fn authenticate_from_auth_header(
    server: &Server,
    header_value: &str,
) -> Result<Authentication> {
    let mut split = header_value.split_whitespace();
    match split.next() {
        Some("Bearer") => match split.next() {
            Some(token) => authenticate_jwt(&server.secrets, token),
            None => bad_request!(r#"missing token after "Bearer""#),
        },
        _ => bad_request!("Could not recognize authentication token type."),
    }
}

/// Authenticate the user performing the request with choosing from one of the authentication method
/// provided by ChiselStrike.
///
/// This method will first look for a JWT in the `Authorization: Bearer` header. If the header is
/// no set, it fallback to the user id provided in the `ChiselUID` header. If nothing is found
/// there, then Authentication::None is returned.
pub async fn authenticate(
    req_parts: &Parts,
    server: &Server,
    version: &Version,
    routing_path: &str,
) -> Result<Authentication> {
    // Check Authorization header first, and process auth if it exists
    let maybe_auth_header = req_parts
        .headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok());
    if let Some(auth_header) = maybe_auth_header {
        let authentication = authenticate_from_auth_header(server, auth_header).await?;
        return Ok(authentication);
    }

    // FIXME: this part of auth is... strange?
    // TODO: we don't authenticate the user!!!
    // get the token here instead, and parse jwt
    let maybe_user_id: Option<String> = req_parts
        .headers
        .get("ChiselUID")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.into());

    let authentication =
        authenticate_by_user_id(server, version, maybe_user_id, routing_path).await?;

    {
        let secrets = server.secrets.read();
        if !version
            .policy_system
            .secret_authorization
            .is_allowed(req_parts, &secrets, routing_path)
        {
            forbidden!("Invalid header authentication");
        }
    }

    Ok(authentication)
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
