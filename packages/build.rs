#![allow(clippy::expect_fun_call)]
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    build_chiselstrike_api();
    build_create_chiselstrike_app();
}

// Build the package `@chiselstrike/api`
fn build_chiselstrike_api() {
    let package_dir = Path::new("./chiselstrike-api").to_path_buf();

    // cleanup the `lib/` directory
    let lib_dir = package_dir.join("lib");
    mkdir_empty(&lib_dir);

    for (name, code) in api::SOURCES_D_TS.iter() {
        let output_path = lib_dir.join(format!("{}.ts.d.ts", name));
        write(&output_path, code);
    }

    println!("cargo:rerun-if-changed=./chiselstrike-api/package.json");
}

// Build the package `create-chiselstrike-app`
fn build_create_chiselstrike_app() {
    let create_app = Path::new("./create-chiselstrike-app").to_path_buf();
    // build create-chiselstrike-app so we can use it in tests
    for v in [
        "create-chiselstrike-app/index.ts",
        "create-chiselstrike-app/template/Chisel.toml",
        "create-chiselstrike-app/template/hello.ts",
        "create-chiselstrike-app/template/package.json",
        "create-chiselstrike-app/template/settings.json",
        "create-chiselstrike-app/template/tsconfig.json",
        "create-chiselstrike-app/tsconfig.json",
        "create-chiselstrike-app/package.json",
    ] {
        println!("cargo:rerun-if-changed=./{}", v);
    }
    run_in("npm", &["install"], create_app.clone());
    run_in("npm", &["run", "build"], create_app);
}

fn run_in(cmd: &str, args: &[&str], dir: PathBuf) {
    assert!(
        dir.exists(),
        "{:?} does not exist. Current directory is {:?}",
        dir,
        env::current_dir().unwrap()
    );
    assert!(dir.is_dir(), "{:?} is not a directory", dir);
    let mut command = Command::new(cmd);
    if openssl_legacy_provider_supported() {
        command.env("NODE_OPTIONS", "--openssl-legacy-provider");
    };
    let status = command.args(args).current_dir(dir.clone()).status();
    assert!(
        status.is_ok(),
        "failed to run command `{}` in dir {:?}, error: {:?}",
        cmd,
        dir,
        status.err().unwrap()
    );
    assert!(status.unwrap().success());
}

fn openssl_legacy_provider_supported() -> bool {
    let status = Command::new("node")
        .env("NODE_OPTIONS", "--openssl-legacy-provider")
        .arg("-v")
        .status();
    status.is_ok() && status.unwrap().success()
}

fn mkdir_empty(dir: &Path) {
    if dir.exists() {
        fs::remove_dir_all(&dir).expect(&format!("Cannot cleanup directory {}", dir.display()));
    }
    fs::create_dir_all(&dir).expect(&format!("Cannot create directory {}", dir.display()));
}

fn write(output: &Path, data: &str) {
    fs::write(output, data).expect(&format!("Cannot write to file {}", output.display()));
}
