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
    assert!(out.starts_with(b" 0c931060784ee83082f7c221374aed451d4f1ad6 deno"));
}
