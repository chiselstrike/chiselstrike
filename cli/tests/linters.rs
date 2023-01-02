// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

mod common;

mod linters {
    use crate::common::{cargo, run};

    #[test]
    fn eslint() {
        run("npm", ["install"]);
        run("npx", ["eslint", ".", "--ext", ".ts"]);
    }

    #[test]
    fn deno_checks() {
        run("deno", ["lint", "--config", "deno.json"]);
        run("deno", ["fmt", "--config", "deno.json", "--check"]);
    }

    #[test]
    fn check_formating() {
        cargo(["fmt", "--", "--check"]);
    }

    #[test]
    fn check_clippy() {
        cargo(["clippy", "--all-targets", "--", "-D", "warnings"]);
    }
}
