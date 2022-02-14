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

mod dbconn;
pub(crate) mod engine;
pub(crate) mod meta;
pub(crate) mod query;

pub(crate) use dbconn::DbConnection;
pub(crate) use dbconn::Kind;
pub(crate) use engine::QueryEngine;
pub(crate) use meta::MetaService;
