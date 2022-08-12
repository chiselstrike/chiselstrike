// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::Opt;
use crate::framework::TestContext;
use futures::future::BoxFuture;
use itertools::iproduct;
use std::sync::Arc;

#[derive(Default)]
pub struct TestSuite {
    tests: Vec<Arc<TestSpec>>,
}

pub struct TestSpec {
    pub name: String,
    pub modules: ModulesSpec,
    pub optimize: OptimizeSpec,
    pub test_fn: &'static (dyn TestFn + Sync),
}

pub struct TestInstance {
    pub spec: Arc<TestSpec>,
    pub modules: Modules,
    pub optimize: bool,
}

impl TestSuite {
    pub fn add(&mut self, spec: TestSpec) {
        self.tests.push(Arc::new(spec));
    }

    pub fn instantiate(&self, opt: &Opt) -> Vec<TestInstance> {
        iproduct!(
            self.tests.iter(),
            [Modules::Deno, Modules::Node],
            [true, false]
        ).filter_map(|(test_spec, modules, optimize)| {
            if let Some(name_regex) = opt.test.as_ref() {
                if !name_regex.is_match(&test_spec.name) {
                    return None
                }
            }

            match (test_spec.modules, modules) {
                (ModulesSpec::Deno, Modules::Deno) => {},
                (ModulesSpec::Node, Modules::Node) => {},
                (ModulesSpec::Both, _) => {},
                (_, _) => return None,
            }

            match (test_spec.optimize, optimize) {
                (OptimizeSpec::Yes, true) => {},
                //(OptimizeSpec::No, false) => {},
                (OptimizeSpec::Both, _) => {},
                (_, _) => return None,
            }

            Some(TestInstance {
                spec: test_spec.clone(),
                modules,
                optimize,
            })
        })
        .collect()
    }
}

impl TestSpec {
    pub fn new(name: &str, modules: ModulesSpec, test_fn: &'static (dyn TestFn + Sync)) -> Self {
        Self {
            name: name.into(),
            modules,
            optimize: OptimizeSpec::Yes,
            test_fn,
        }
    }

    pub fn deno(name: &str, test_fn: &'static (dyn TestFn + Sync)) -> Self {
        Self::new(name, ModulesSpec::Deno, test_fn)
    }

    pub fn node(name: &str, test_fn: &'static (dyn TestFn + Sync)) -> Self {
        Self::new(name, ModulesSpec::Node, test_fn)
    }

    pub fn optimize(mut self, optimize: OptimizeSpec) -> Self {
        self.optimize = optimize;
        self
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

