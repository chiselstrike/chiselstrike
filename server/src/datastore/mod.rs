// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

//! # Query Engine
//!
//! ## Requirements
//!
//! - ChiselStrike has a Query and Entity API that endpoints use to persist
//!   and retrieve entities.
//! - Query execution must be efficient, but respect the declarative policies
//!   set by the user.
//!
//! ## Design
//!
//! The high-level Query and Entity API that endpoints use is written in
//! TypeScript.
//!
//! For example, an developer would define a blog post entity as follows:
//!
//! ```ignore
//! export class Post extends Entity {
//!     title: string;
//!     body: string;
//! }
//! ```
//!
//! and then they would be able to persist and retrieve entities with:
//!
//! ```ignore
//! Entity.create({ title: 'hello, world!', body: 'Lorem impsum' });
//!
//! Entity.every().filter({ title: 'hello, world!' })
//! ```
//!
//! The TypeScript API transforms these **mutations** and **queries** into
//! *query expressions,* which is a JSON format that is passed to the query
//! engine via a Deno op.
//!
//! The ``QueryEngine`` has the following high-level API:
//!
//! ```ignore
//! fn mutate(Mutation) -> Result<();
//!
//! fn query(Query) -> Result<QueryResults>;
//! ```
//!
//! The `mutate` method mutates the underlying backing store state as per the
//! `Mutation` object. For example, if the developer calls the
//! `Entity.delete()` method in TypeScript the query engine sees a `Mutation`
//! object that describes a SQL `DELETE` statement.
//!
//! The `query` method is similar to `mutate`, but it works on a `Query`
//! object instead and returns a `QueryResults` object, which represents a
//! stream of query results with *policies applied*.

pub mod crud;
mod dbconn;
pub mod engine;
pub mod expr;
pub mod meta;
pub mod query;

use std::collections::HashMap;
use std::task::Poll;

pub use dbconn::DbConnection;
pub use engine::QueryEngine;
pub use meta::MetaService;

use deno_core::futures::{self, StreamExt};
use serde_json::Value as JsonValue;

use self::engine::QueryResults;
use crate::policy::engine::{Action, PolicyEvalInstance};
use crate::types::Entity;

struct EntityStream {
    base_type: Entity,
    inner: QueryResults,
    policy_instances: HashMap<String, PolicyEvalInstance>,
}

impl EntityStream {
    fn validate(
        &self,
        value: serde_json::Map<String, JsonValue>,
    ) -> anyhow::Result<Option<serde_json::Map<String, JsonValue>>> {
        match self.policy_instances.get(self.base_type.name()) {
            Some(instance) => match instance.get_read_action(&self.base_type, &value)? {
                Action::Allow => Ok(Some(value)),
                Action::Deny => Err(anyhow::anyhow!("access denied")),
                Action::Skip => Ok(None),
                Action::Log => {
                    info!("json value: {:?}", value);
                    Ok(Some(value))
                }
            },
            // no policy, don't do anything.
            None => Ok(Some(value)),
        }
    }
}

impl futures::Stream for EntityStream {
    type Item = anyhow::Result<serde_json::Map<String, JsonValue>>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // we "could" be dropping many elements here, to avoid blocking the main loop for too long,
        // we limit the amount of items that get can get at once, and collaboratively yield.
        for _ in 0..64 {
            let item = futures::ready!(self.inner.poll_next_unpin(cx));
            match item {
                Some(item) => match item {
                    Ok(value) => match self.validate(value) {
                        Ok(Some(value)) => return Poll::Ready(Some(Ok(value))),
                        Ok(None) => continue,
                        Err(e) => return Poll::Ready(Some(Err(e))),
                    },
                    Err(e) => return Poll::Ready(Some(Err(e))),
                },
                None => return Poll::Ready(None),
            }
        }

        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
