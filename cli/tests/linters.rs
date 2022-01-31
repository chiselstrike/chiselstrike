// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

mod common;

#[cfg(test)]
mod tests {
    use crate::common::run;

    fn cargo_install(version: &'static str, pkg: &'static str, bin: &'static str) {
        run(
            "cargo",
            [
                "install",
                "--version",
                version,
                pkg,
                "--bin",
                bin,
                "--locked",
            ],
        );
    }

    #[test]
    fn eslint() {
        run("npm", ["install"]);
        run("npx", ["eslint", ".", "--ext", ".ts"]);
    }

    #[test]
    fn deno_checks() {
        cargo_install("1.16.3", "deno", "deno");
        run("deno", ["lint", "--config", "deno.json"]);
        run("deno", ["fmt", "--config", "deno.json", "--check"]);
    }

    #[test]
    fn sorted_dependencies() {
        cargo_install("1.0.5", "cargo-sort", "cargo-sort");
        run("cargo", ["sort", "-w", "-c"]);
    }

    #[test]
    fn unused_dependencies() {
        cargo_install("0.1.26", "cargo-udeps", "cargo-udeps");
        run("cargo", ["+nightly", "udeps"]);
    }

    #[test]
    fn check_formating() {
        run("cargo", ["fmt", "--all", "--", "--check"]);
    }

    #[test]
    fn must_not_suspend() {
        run(
            "cargo",
            ["+nightly", "check", "--features", "must_not_suspend"],
        );
    }

    #[test]
    fn check_clippy() {
        run("cargo", ["clippy", "--all-targets", "--", "-D", "warnings"]);
    }
}
