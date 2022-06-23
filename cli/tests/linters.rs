// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

mod common;

#[cfg(test)]
mod tests {
    use crate::common::{repo_dir, run, Command};
    use once_cell::sync::Lazy;
    use std::ffi::OsString;
    use std::fs::{create_dir_all, File};
    use std::io::Read;
    use std::path::PathBuf;
    use toml::Value;

    static CHISEL_BIN_CACHE: Lazy<PathBuf> = Lazy::new(|| repo_dir().join(".chisel_dev"));
    static CHISEL_CARGO_PATH: Lazy<OsString> = Lazy::new(|| {
        let new_path = format!("{}/bin:{}", CHISEL_BIN_CACHE.display(), env!("PATH"));
        OsString::from(new_path)
    });

    fn cargo<'a, T: IntoIterator<Item = &'a str>>(args: T) -> Command {
        run("cargo", args, Some(&CHISEL_CARGO_PATH))
    }

    fn nightly<'a, T: IntoIterator<Item = &'a str>>(args: T) -> Command {
        let mut ret = cargo(itertools::chain(["+nightly-2022-03-15"], args));
        ret.env("CARGO_TARGET_DIR", "./target/nightly");
        ret
    }

    fn cargo_install(version: &str, pkg: &str, bin: &str) {
        create_dir_all(&*CHISEL_BIN_CACHE).unwrap();
        cargo([
            "install",
            "--version",
            version,
            pkg,
            "--bin",
            bin,
            "--locked",
            "--root",
            &CHISEL_BIN_CACHE.display().to_string(),
        ]);
    }

    #[test]
    fn eslint() {
        run("npm", ["install"], None);
        run("npx", ["eslint", ".", "--ext", ".ts"], None);
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
        run("deno", ["lint", "--config", "deno.json"], None);
        run("deno", ["fmt", "--config", "deno.json", "--check"], None);
    }

    #[test]
    fn sorted_dependencies() {
        cargo_install("1.0.5", "cargo-sort", "cargo-sort");
        cargo(["sort", "-w", "-c"]);
    }

    #[test]
    fn unused_dependencies() {
        cargo_install("0.1.29", "cargo-udeps", "cargo-udeps");
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
