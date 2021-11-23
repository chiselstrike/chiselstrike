// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::types::TypeSystemError;

mod dbconn;
pub mod engine;
pub mod meta;

#[derive(thiserror::Error, Debug)]
pub enum QueryError {
    #[error["connection failed `{0}`"]]
    ConnectionFailed(#[source] sqlx::Error),
    #[error["execution failed: `{0}`"]]
    ExecuteFailed(#[source] sqlx::Error),
    #[error["fetch failed `{0}`"]]
    FetchFailed(#[source] sqlx::Error),
    #[error["row parsing failed `{0}`"]]
    ParsingFailed(#[source] sqlx::Error),
    #[error["type system error `{0}`"]]
    TypeError(#[from] TypeSystemError),
    #[error["type `{0}` has no field `{1}`"]]
    UnknownField(String, String),
    #[error["provided data for field `{0}` are incompatible with given type `{1}`"]]
    IncompatibleData(String, String),
    #[error["feature `{0}` is not implemented"]]
    NotImplemented(String),
}

pub use dbconn::DbConnection;
pub use dbconn::Kind;
pub use engine::QueryEngine;
pub use meta::MetaService;
