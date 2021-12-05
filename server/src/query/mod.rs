// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::types::TypeSystemError;

mod dbconn;
pub(crate) mod engine;
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
    #[error["type `{0}` has no field `{1}`"]]
    UnknownField(String, String),
    #[error["provided data for field `{0}` are incompatible with given type `{1}`"]]
    IncompatibleData(String, String),
    #[error["feature `{0}` is not implemented"]]
    NotImplemented(String),
    #[error["token '{0}' not found"]]
    TokenNotFound(String),
}

pub(crate) use dbconn::DbConnection;
pub(crate) use dbconn::Kind;
pub(crate) use engine::QueryEngine;
pub(crate) use meta::MetaService;
