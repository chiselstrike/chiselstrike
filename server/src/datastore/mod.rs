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
use crate::policy::PolicyContext;
use crate::types::TypeSystem;

use self::engine::TransactionStatic;

pub struct DataContext {
    pub type_system: Arc<TypeSystem>,
    pub policy_system: Arc<PolicySystem>,
    pub job_info: Rc<JobInfo>,
    pub policy_context: Rc<PolicyContext>,
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

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use futures::Future;
    use once_cell::sync::Lazy;
    use url::Url;

    use crate::{
        types::{self, Entity, Field, ObjectType, Type},
        JsonObject,
    };

    use super::{crud::QueryParams, *};

    pub const VERSION: &str = "version_1";

    pub fn make_type_system(entities: &[Entity]) -> TypeSystem {
        let builtin = Arc::new(types::BuiltinTypes::new());
        let mut ts = TypeSystem::new(builtin, VERSION.into());
        for ty in entities {
            ts.add_custom_type(ty.clone()).unwrap();
        }
        ts
    }

    pub fn make_entity(name: &str, fields: Vec<Field>) -> Entity {
        let desc = types::NewObject::new(name, VERSION);
        Entity::Custom(Arc::new(ObjectType::new(&desc, fields, vec![]).unwrap()))
    }

    pub fn make_field(name: &str, ty: Type) -> Field {
        let desc = types::NewField::new(name, ty, VERSION).unwrap();
        Field::new(&desc, vec![], None, false, false)
    }

    pub static PERSON_TY: Lazy<Entity> = Lazy::new(|| {
        make_entity(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        )
    });

    pub static COMPANY_TY: Lazy<Entity> = Lazy::new(|| {
        make_entity(
            "Company",
            vec![
                make_field("name", Type::String),
                make_field("ceo", PERSON_TY.clone().into()),
            ],
        )
    });

    pub static ENTITIES: Lazy<[Entity; 2]> = Lazy::new(|| [PERSON_TY.clone(), COMPANY_TY.clone()]);
    pub static TYPE_SYSTEM: Lazy<TypeSystem> = Lazy::new(|| make_type_system(&*ENTITIES));

    impl QueryEngine {
        /// creates a dummy context with a transaction, executes f, and then attemps to commit the
        /// transaction.
        ///
        /// f takes ownership of the context and return it so we don't have to deal with closure
        /// returning futures borrowing their environment
        pub async fn with_dummy_ctx<Fut>(
            &self,
            headers: HashMap<String, String>,
            f: impl FnOnce(DataContext) -> Fut,
        ) where
            Fut: Future<Output = DataContext>,
        {
            let job_info = Rc::new(JobInfo::HttpRequest {
                method: "POST".into(),
                path: "".into(),
                headers,
                user_id: None,
                response_tx: Default::default(),
            });
            let policy_context = PolicyContext {
                cache: Default::default(),
                engine: Default::default(),
                request: job_info.clone(),
            };
            let ctx = self
                .create_data_context(
                    Arc::new(TYPE_SYSTEM.clone()),
                    Default::default(),
                    policy_context,
                    job_info.clone(),
                )
                .await
                .unwrap();

            let ctx = f(ctx).await;

            QueryEngine::commit_transaction_static(ctx.txn)
                .await
                .unwrap();
        }

        pub async fn run_test_query(
            &self,
            ctx: &DataContext,
            entity_name: &str,
            url: Url,
        ) -> anyhow::Result<JsonObject> {
            self.run_query(
                ctx,
                QueryParams {
                    type_name: entity_name.to_owned(),
                    url_path: url.path().to_owned(),
                    url_query: url.query_pairs().into_owned().collect(),
                },
            )
            .await
        }

        pub async fn run_query_vec(
            &self,
            ctx: &DataContext,
            entity_name: &str,
            url: Url,
        ) -> Vec<String> {
            let r = self.run_test_query(ctx, entity_name, url).await.unwrap();
            collect_names(&r)
        }
    }

    pub fn collect_names(r: &JsonObject) -> Vec<String> {
        r["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect()
    }
}
