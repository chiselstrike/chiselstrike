// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::types::TypeSystemError;

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
    #[error["type system error `{0}`"]]
    TypeError(#[from] TypeSystemError),
    #[error["provided data are incompatible with given type `{0}`"]]
    IncompatibleData(String),
}

pub use engine::QueryEngine;
pub use meta::MetaService;
