// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

pub use crate::suite::{TestSuite, TestSpec, ModulesSpec, OptimizeSpec};

mod bad_filter;
mod find_by;
mod http_import;
mod routing;

pub fn suite() -> TestSuite {
    let mut suite = TestSuite::default();
    suite.add(TestSpec::deno("bad_filter", &bad_filter::test));
    suite.add(TestSpec::deno("find_by", &find_by::test).optimize(OptimizeSpec::Both));
    suite.add(TestSpec::node("http_import", &http_import::test));
    suite.add(TestSpec::new("routing::basic", ModulesSpec::Both, &routing::basic));
    suite.add(TestSpec::new("routing::params_in_code", ModulesSpec::Deno, &routing::params_in_code));
    suite.add(TestSpec::new("routing::params_in_files", ModulesSpec::Both, &routing::params_in_files));
    suite.add(TestSpec::new("routing::params_get_typed", ModulesSpec::Deno, &routing::params_get_typed));
    suite.add(TestSpec::new("routing::params_get_wrong", ModulesSpec::Deno, &routing::params_get_wrong));
    suite.add(TestSpec::new("routing::slashes", ModulesSpec::Deno, &routing::slashes));
    suite.add(TestSpec::new("routing::method_shorthands", ModulesSpec::Deno, &routing::method_shorthands));
    suite.add(TestSpec::new("routing::method_manual", ModulesSpec::Deno, &routing::method_manual));
    suite.add(TestSpec::new("routing::method_object", ModulesSpec::Deno, &routing::method_object));
    suite.add(TestSpec::new("routing::errors", ModulesSpec::Deno, &routing::errors));
    suite
}

