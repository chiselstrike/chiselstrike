// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

mod common;

#[cfg(test)]
mod tests {
    use crate::common::{run, Command};

    fn cargo<'a, T: IntoIterator<Item = &'a str>>(args: T) -> Command {
        run("cargo", args)
    }

    fn nightly<'a, T: IntoIterator<Item = &'a str>>(args: T) -> Command {
        let mut ret = cargo(itertools::chain(["+nightly-2022-03-15"], args));
        ret.env("CARGO_TARGET_DIR", "./target/nightly");
        ret
    }

    fn cargo_install(version: &str, pkg: &str, bin: &str) {
        cargo([
            "install",
            "--version",
            version,
            pkg,
            "--bin",
            bin,
            "--locked",
        ]);
    }

    #[test]
    fn eslint() {
        run("npm", ["install"]);
        run("npx", ["eslint", ".", "--ext", ".ts"]);
    }

    #[test]
    fn deno_checks() {
        cargo([
            "install",
            "--path",
            "./third_party/deno/cli",
            "--bin",
            "deno",
            "--locked",
        ])
        .env("CARGO_TARGET_DIR", "./target");
        run("deno", ["lint", "--config", "deno.json"]);
        run("deno", ["fmt", "--config", "deno.json", "--check"]);
    }

    #[test]
    fn sorted_dependencies() {
        cargo_install("1.0.5", "cargo-sort", "cargo-sort");
        cargo(["sort", "-w", "-c"]);
    }

    #[test]
    fn unused_dependencies() {
        cargo_install("0.1.26", "cargo-udeps", "cargo-udeps");
        nightly(["udeps"]);
    }

    #[test]
    fn check_formating() {
        cargo(["fmt", "--all", "--", "--check"]);
    }

    #[test]
    fn must_not_suspend() {
        nightly(["check", "--features", "must_not_suspend"]);
    }

    #[test]
    fn check_clippy() {
        cargo(["clippy", "--all-targets", "--", "-D", "warnings"]);
    }
}
