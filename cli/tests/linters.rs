// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

mod common;

#[cfg(test)]
mod tests {
    use crate::common::run;

    #[test]
    fn deno_checks() {
        run("cargo", ["install", "deno", "--bin", "deno"]);
        run("deno", ["lint", "--config", "deno.json"]);
        run("deno", ["fmt", "--config", "deno.json", "--check"]);
    }

    #[test]
    fn sorted_dependencies() {
        run(
            "cargo",
            [
                "install",
                "--version",
                "1.0.5",
                "cargo-sort",
                "--bin",
                "cargo-sort",
            ],
        );
        run("cargo", ["sort", "-w", "-c"]);
    }

    #[test]
    fn check_formating() {
        run("cargo", ["fmt", "--all", "--", "--check"]);
    }

    #[test]
    fn check_clippy() {
        run(
            "cargo",
            [
                "clippy",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
        );
    }
}
