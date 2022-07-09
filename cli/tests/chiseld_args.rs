mod common;

use std::process::Command;

use common::chisel_exe;

fn run_chisel(args: &[&str]) -> serde_json::Value {
    let out = Command::new(chisel_exe()).args(args).output().unwrap();

    let stdout = std::str::from_utf8(&out.stdout).unwrap();

    // skip welcome message, and extract json
    let i = stdout.char_indices().find(|(_, c)| *c == '{').unwrap().0;
    let out = &stdout[i..];

    serde_json::from_str(out).unwrap()
}

#[test]
fn start_pass_args_to_chiseld() {
    let json = run_chisel(&[
        "start",
        "--",
        "--api-listen-addr",
        "addr:12345",
        "--show-config",
    ]);

    assert_eq!(json["api_listen_addr"], "addr:12345");
}

#[test]
fn dev_pass_args_to_chiseld() {
    let json = run_chisel(&[
        "dev",
        "--",
        "--api-listen-addr",
        "addr:12345",
        "--show-config",
    ]);

    assert_eq!(json["api_listen_addr"], "addr:12345");
}
