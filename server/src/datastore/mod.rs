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
pub mod value;

use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
pub use dbconn::DbConnection;
pub use engine::QueryEngine;
pub use meta::MetaService;

use crate::ops::job_context::JobInfo;
use crate::policies::PolicySystem;
use crate::types::TypeSystem;

use self::engine::TransactionStatic;

pub struct DataContext {
    pub type_system: Arc<TypeSystem>,
    pub policy_system: Arc<PolicySystem>,
    pub job_info: Rc<JobInfo>,

    pub txn: TransactionStatic,
}

impl DataContext {
    pub async fn commit(self) -> anyhow::Result<()> {
        let transaction = Arc::try_unwrap(self.txn)
            .ok()
            .context(
                "Cannot commit a transaction because there is an operation \
            in progress that uses this transaction",
            )?
            .into_inner();
        QueryEngine::commit_transaction(transaction).await?;

        Ok(())
    }

    pub fn rollback(self) -> anyhow::Result<()> {
        let transaction = Arc::try_unwrap(self.txn)
            .ok()
            .context(
                "Cannot rollback transaction because there is an operation \
            in progress that uses this transaction",
            )?
            .into_inner();

        drop(transaction);

        Ok(())
    }
}
