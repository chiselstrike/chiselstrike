use anyhow::Context;
// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
use http::request::Parts;
use jsonwebtoken::{DecodingKey, Validation};
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::error::{Result, ResultExt};
use crate::JsonObject;

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

impl Authentication {
    pub fn user_id(&self) -> Option<&str> {
        // TODO: maybe extract uid from JWT if JWT contains a `userId` field?
        match self {
            Authentication::UserId(ref uid) => Some(uid),
            _ => None,
        }
    }
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
                .context("JWT signing key is not valid UTF-8")
                .err_internal()?;
            buf.push_str(line);
            buf.push('\n');
            Ok(buf)
        })?;
    key.push_str("-----END PUBLIC KEY-----");

    let dkey = DecodingKey::from_rsa_pem(key.as_bytes()).err_internal()?;

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
    let secret_value = match lock.get("CHISEL_JWT_VALIDATION_KEY") {
        Some(key) => key,
        None => internal!(
            "Missing jwt validation key: please set `CHISEL_JWT_VALIDATION_KEY` in your secrets"
        ),
    };
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
    let token = jsonwebtoken::decode::<JsonValue>(
        token,
        &key,
        &Validation::new(jsonwebtoken::Algorithm::RS256),
    )
    .err_forbidden()?;

    Ok(token.claims)
}

async fn authenticate_from_auth_header(
    secrets: &RwLock<JsonObject>,
    header_value: &str,
) -> Result<Authentication> {
    let mut split = header_value.split_whitespace();
    match split.next() {
        Some("Bearer") => match split.next() {
            Some(token) => authenticate_jwt(secrets, token),
            None => bad_request!(r#"missing token after "Bearer""#),
        },
        _ => {
            bad_request!("Could not recognize authentication token type: expected `Bearer` token.")
        }
    }
}

/// Authenticate the user performing the request by choosing from one of the authentication method
/// provided by ChiselStrike.
///
/// This method will first look for a JWT in the `Authorization: Bearer` header. If the header
/// isn't set, it falls back to the user_id provided in the `ChiselUID` header. If nothing is found
/// there, then Authentication::None is returned.
pub async fn authenticate(
    req_parts: &Parts,
    secrets: &RwLock<JsonObject>,
) -> Result<Authentication> {
    // Check Authorization header first, and process auth if it exists
    let maybe_auth_header = req_parts
        .headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok());
    if let Some(auth_header) = maybe_auth_header {
        let authentication = authenticate_from_auth_header(secrets, auth_header).await?;
        return Ok(authentication);
    }

    // TODO: we don't authenticate the user!!!
    // get the token here instead, and parse jwt
    let maybe_user_id: Option<String> = req_parts
        .headers
        .get("ChiselUID")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.into());

    Ok(maybe_user_id.map_or(Authentication::None, Authentication::UserId))
}
