use std::fs::create_dir_all;
use std::io::Write;
use std::path::PathBuf;

use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

fn chiseld() -> PathBuf {
    let mut current_exe = std::env::current_exe().unwrap();
    current_exe.pop();
    current_exe.pop();
    current_exe.join("chiseld")
}

fn chiseld_check_config(args: &[&str], env: &[(&str, &str)]) -> serde_json::Value {
    let mut cmd = std::process::Command::new(chiseld());

    cmd.args(args);
    cmd.envs(env.iter().cloned());
    cmd.arg("--show-config");

    let out = cmd.output().unwrap();

    let stderr = std::str::from_utf8(&out.stderr).unwrap();
    println!("{stderr}");

    assert!(out.status.success());

    serde_json::from_slice(&out.stdout).unwrap()
}

fn write_config(s: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(s.as_bytes()).unwrap();
    file.flush().unwrap();

    file
}

#[allow(dead_code)]
fn write_config_default_path(s: &str) -> TempDir {
    let tmp_dir = tempfile::tempdir().unwrap();

    let chisel_dir = tmp_dir.path().join("chiselstrike");
    create_dir_all(&chisel_dir).unwrap();

    std::fs::write(chisel_dir.join("config.toml"), s.as_bytes()).unwrap();

    tmp_dir
}

#[test]
fn test_simple_config_file() {
    let conf = write_config(
        r#"
api_listen_addr = "localhost:12345"
    "#,
    );
    let out = chiseld_check_config(&["-c", &conf.path().display().to_string()], &[]);

    let expected = serde_json::json!({
        "api_listen_addr": "localhost:12345",
        "rpc_listen_addr": "127.0.0.1:50051",
        "internal_routes_listen_addr": "127.0.0.1:9090",
        "_metadata_db_uri": "sqlite://chiseld.db?mode=rwc",
        "_data_db_uri": "sqlite://chiseld-data.db?mode=rwc",
        "db_uri": "sqlite://.chiseld.db?mode=rwc",
        "kafka_connection": Value::Null,
        "kafka_topics": Value::Array(vec![]),
        "v8_flags": Value::Array(vec![]),
        "inspect": false,
        "inspect_brk": false,
        "debug": false,
        "nr_connections": 10,
        "worker_threads": 21,
        "chisel_secret_location": Value::Null,
        "chisel_secret_key_location": Value::Null,
    });

    assert_eq!(out, expected);
}

#[test]
fn test_simple_cli_priority() {
    let conf = write_config(
        r#"
api_listen_addr = "localhost:12345"
    "#,
    );
    let out = chiseld_check_config(
        &[
            "-c",
            &conf.path().display().to_string(),
            "--api-listen-addr",
            "localhost:123457",
        ],
        &[],
    );

    let expected = serde_json::json!({
        "api_listen_addr": "localhost:123457",
        "rpc_listen_addr": "127.0.0.1:50051",
        "internal_routes_listen_addr": "127.0.0.1:9090",
        "_metadata_db_uri": "sqlite://chiseld.db?mode=rwc",
        "_data_db_uri": "sqlite://chiseld-data.db?mode=rwc",
        "db_uri": "sqlite://.chiseld.db?mode=rwc",
        "kafka_connection": Value::Null,
        "kafka_topics": Value::Array(vec![]),
        "v8_flags": Value::Array(vec![]),
        "inspect": false,
        "inspect_brk": false,
        "debug": false,
        "nr_connections": 10,
        "worker_threads": 21,
        "chisel_secret_location": Value::Null,
        "chisel_secret_key_location": Value::Null,
    });

    assert_eq!(out, expected);
}

#[test]
// we can only run this test on linux, since we are setting XDG_CONFIG_HOME
#[cfg(target_os = "linux")]
fn test_default_file_location() {
    let conf = write_config_default_path(
        r#"
api_listen_addr = "localhost:12345"
    "#,
    );

    let out = chiseld_check_config(
        &[],
        &[("XDG_CONFIG_HOME", &conf.path().display().to_string())],
    );

    let expected = serde_json::json!({
        "api_listen_addr": "localhost:12345",
        "rpc_listen_addr": "127.0.0.1:50051",
        "internal_routes_listen_addr": "127.0.0.1:9090",
        "_metadata_db_uri": "sqlite://chiseld.db?mode=rwc",
        "_data_db_uri": "sqlite://chiseld-data.db?mode=rwc",
        "db_uri": "sqlite://.chiseld.db?mode=rwc",
        "kafka_connection": Value::Null,
        "kafka_topics": Value::Array(vec![]),
        "v8_flags": Value::Array(vec![]),
        "inspect": false,
        "inspect_brk": false,
        "debug": false,
        "nr_connections": 10,
        "worker_threads": 21,
        "chisel_secret_location": Value::Null,
        "chisel_secret_key_location": Value::Null,
    });

    assert_eq!(out, expected);
}

#[test]
// we can only run this test on linux, since we are setting XDG_CONFIG_HOME
#[cfg(target_os = "linux")]
fn exlicit_config_file_over_default() {
    let default_conf = write_config_default_path(
        r#"
api_listen_addr = "localhost:12345"
    "#,
    );

    let explicit_conf = write_config(
        r#"
api_listen_addr = "localhost:12346"
    "#,
    );

    let out = chiseld_check_config(
        &["-c", &explicit_conf.path().display().to_string()],
        &[(
            "XDG_CONFIG_HOME",
            &default_conf.path().display().to_string(),
        )],
    );

    let expected = serde_json::json!({
        "api_listen_addr": "localhost:12346",
        "rpc_listen_addr":"127.0.0.1:50051",
        "internal_routes_listen_addr":"127.0.0.1:9090",
        "_metadata_db_uri":"sqlite://chiseld.db?mode=rwc",
        "_data_db_uri":"sqlite://chiseld-data.db?mode=rwc",
        "db_uri":"sqlite://.chiseld.db?mode=rwc",
        "kafka_connection": Value::Null,
        "kafka_topics": Value::Array(vec![]),
        "v8_flags": Value::Array(vec![]),
        "inspect": false,
        "inspect_brk":false,
        "debug": false,
        "nr_connections":10,
        "worker_threads": 21,
        "chisel_secret_location": Value::Null,
        "chisel_secret_key_location": Value::Null,
    });

    assert_eq!(out, expected);
}
