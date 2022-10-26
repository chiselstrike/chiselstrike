// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

mod common;

mod linters {
    use crate::common::{repo_dir, run, Command, CHISEL_BIN_DIR};
    use regex::Regex;
    use std::fs::{create_dir_all, File};
    use std::io::Read;
    use toml::Value;

    fn cargo<'a, T: IntoIterator<Item = &'a str>>(args: T) -> Command {
        run("cargo", args)
    }

    fn nightly<'a, T: IntoIterator<Item = &'a str>>(args: T) -> Command {
        // the nightly version needs to be kept in sync with .github/workflows/rust.yml
        let mut ret = cargo(itertools::chain(["+nightly-2022-10-24"], args));
        ret.env("CARGO_TARGET_DIR", "./target/nightly");
        ret
    }

    fn cargo_install(version: &str, pkg: &str, bin: &str) {
        ensure_correct_version(pkg, version);
        create_dir_all(&*CHISEL_BIN_DIR).unwrap();
        cargo([
            "install",
            "--version",
            version,
            pkg,
            "--bin",
            bin,
            "--locked",
            "--root",
            &CHISEL_BIN_DIR.display().to_string(),
        ]);
    }

    fn ensure_correct_version(pkg: &str, version: &str) {
        let re = Regex::new(&format!(r"{pkg} v{version}:")).unwrap();
        let mut cmd = cargo(["install", "--list"]);
        let out = cmd.inner.output().unwrap();

        let stdout = std::str::from_utf8(&out.stdout).unwrap();

        // only accept global package if it's the correct version.
        assert!(re.is_match(stdout) || !stdout.contains(pkg),
            "An incorrect version of {pkg} is present in the global environment. Please uninstall it.");
    }

    #[test]
    fn eslint() {
        run("npm", ["install"]);
        run("npx", ["eslint", ".", "--ext", ".ts"]);
    }

    fn get_deno_version() -> String {
        let mut f = File::open(repo_dir().join("third_party/deno/cli/Cargo.toml")).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        let doc = s.parse::<Value>().unwrap();
        if let Value::String(v) = &doc["package"]["version"] {
            return v.clone();
        }
        panic!("Could not find the deno version");
    }

    #[test]
    fn deno_checks() {
        // Find our deno version and install that. We don't use --path
        // because that always reinstall the binary.
        let version = get_deno_version();
        cargo_install(&version, "deno", "deno");
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
        cargo_install("0.1.34", "cargo-udeps", "cargo-udeps");
        nightly(["udeps"]);
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
