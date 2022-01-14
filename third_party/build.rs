use std::process::Command;

fn main() {
    Command::new("git")
        .args(["submodule", "update", "--init"])
        .status()
        .unwrap();
}
