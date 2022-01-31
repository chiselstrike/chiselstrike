use std::process::Command;

fn main() {
    Command::new("git")
        .args(["submodule", "update", "--init"])
        .status()
        .unwrap();

    let out = Command::new("git")
        .args(["submodule", "status"])
        .output()
        .unwrap()
        .stdout;

    // Check that we got the correct revision. The main reason for
    // this is to force build.rs to change and the build to be rerun
    // when updating the submodule.
    assert!(out.starts_with(b" df5fe5a35f8ace40275a822927f3385c828beba7 deno"));
}
