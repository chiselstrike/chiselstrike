// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

mod common;

#[cfg(test)]
mod tests {
    use crate::common::repo_dir;
    use std::process::Command;

    #[test]
    fn sorted_dependencies() {
        let repo = repo_dir();
        let status = Command::new("cargo")
            .args([
                "install",
                "--version",
                "1.0.5",
                "cargo-sort",
                "--bin",
                "cargo-sort",
            ])
            .current_dir(repo.clone())
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("cargo")
            .args(["sort", "-w", "-c"])
            .current_dir(repo)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn check_formating() {
        let status = Command::new("cargo")
            .args(["fmt", "--all", "--", "--check"])
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn check_clippy() {
        let status = Command::new("cargo")
            .args([
                "clippy",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
}
