// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::types::TypeSystemError;

mod dbconn;
pub(crate) mod engine;
pub(crate) mod expr;
pub(crate) mod meta;

#[derive(thiserror::Error, Debug)]
pub(crate) enum QueryError {
    #[error["connection failed `{0}`"]]
    ConnectionFailed(#[source] sqlx::Error),
    #[error["execution failed: `{0}`"]]
    ExecuteFailed(#[source] sqlx::Error),
    #[error["fetch failed `{0}`"]]
    FetchFailed(#[source] sqlx::Error),
    #[error["type system error `{0}`"]]
    TypeError(#[from] TypeSystemError),
    #[error["provided data for field `{0}` are incompatible with given type `{1}`"]]
    IncompatibleData(String, String),
    #[error["token '{0}' not found"]]
    TokenNotFound(String),
    #[error["invalid ID '{0}'"]]
    InvalidId(String),
}

pub(crate) use dbconn::DbConnection;
pub(crate) use dbconn::Kind;
pub(crate) use engine::QueryEngine;
pub(crate) use meta::MetaService;
