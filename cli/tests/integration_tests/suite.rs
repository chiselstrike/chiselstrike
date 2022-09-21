// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::database::{DatabaseConfig, PostgresConfig};
use crate::framework::TestContext;
use crate::{DatabaseKind, Opt};
use futures::future::BoxFuture;
use itertools::iproduct;

#[derive(Default)]
pub struct TestSuite {
    tests: Vec<&'static TestSpec>,
}

pub struct TestSpec {
    pub name: &'static str,
    pub modules: ModulesSpec,
    pub optimize: OptimizeSpec,
    pub db: DatabaseSpec,
    pub start_chiseld: bool,
    pub test_fn: &'static (dyn TestFn + Sync),
}

pub struct TestInstance {
    pub spec: &'static TestSpec,
    pub modules: Modules,
    pub optimize: bool,
    pub db_config: DatabaseConfig,
}

inventory::collect!(TestSpec);

impl TestSuite {
    pub fn from_inventory() -> Self {
        Self {
            tests: inventory::iter::<TestSpec>.into_iter().collect(),
        }
    }

    pub fn instantiate(&self, opt: &Opt) -> Vec<TestInstance> {
        iproduct!(
            self.tests.iter(),
            [Modules::Deno, Modules::Node],
            [true, false]
        )
        .filter_map(|(test_spec, modules, optimize)| {
            if let Some(name_regex) = opt.test.as_ref().or(opt.test_arg.as_ref()) {
                if !name_regex.is_match(test_spec.name) {
                    return None;
                }
            }

            match (test_spec.modules, modules) {
                (ModulesSpec::Deno, Modules::Deno) => {}
                (ModulesSpec::Node, Modules::Node) => {}
                (ModulesSpec::Both, _) => {}
                (_, _) => return None,
            }

            match (test_spec.optimize, optimize) {
                (OptimizeSpec::Yes, true) => {}
                //(OptimizeSpec::No, false) => {},
                (OptimizeSpec::Both, _) => {}
                (_, _) => return None,
            }

            let db_config = match (test_spec.db, opt.database) {
                (DatabaseSpec::Any | DatabaseSpec::Sqlite, DatabaseKind::Sqlite) => {
                    DatabaseConfig::Sqlite
                }
                (DatabaseSpec::Any, DatabaseKind::Postgres) => {
                    DatabaseConfig::Postgres(PostgresConfig::new(
                        opt.database_host.clone(),
                        opt.database_user.clone(),
                        opt.database_password.clone(),
                    ))
                }
                (DatabaseSpec::LegacySplitSqlite, DatabaseKind::Sqlite) => {
                    DatabaseConfig::LegacySplitSqlite
                }
                (DatabaseSpec::Sqlite, DatabaseKind::Postgres) => return None,
                (DatabaseSpec::LegacySplitSqlite, DatabaseKind::Postgres) => return None,
            };

            Some(TestInstance {
                spec: test_spec,
                modules,
                optimize,
                db_config,
            })
        })
        .collect()
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ModulesSpec {
    Deno,
    Node,
    Both,
}

#[derive(Copy, Clone, Debug)]
pub enum Modules {
    Deno,
    Node,
}

#[derive(Copy, Clone, Debug)]
pub enum OptimizeSpec {
    Yes,
    //No,
    Both,
}

#[derive(Copy, Clone, Debug)]
pub enum DatabaseSpec {
    Any,
    Sqlite,
    LegacySplitSqlite,
}

pub trait TestFn {
    fn call(&self, args: TestContext) -> BoxFuture<'static, ()>;
}

impl<T, F> TestFn for T
where
    T: Fn(TestContext) -> F,
    F: std::future::Future<Output = ()> + 'static + std::marker::Send,
{
    fn call(&self, ctx: TestContext) -> BoxFuture<'static, ()> {
        Box::pin(self(ctx))
    }
}
