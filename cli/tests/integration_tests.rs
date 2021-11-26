// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

mod common;

#[cfg(test)]
mod tests {
    use crate::common::bin_dir;
    use crate::common::repo_dir;
    use ntest::timeout;
    use std::env;
    use std::process::Command;

    fn chisel() -> String {
        bin_dir().join("chisel").to_str().unwrap().to_string()
    }

    #[test]
    #[timeout(100_000)]
    fn lit() {
        let repo = repo_dir();
        let bd = bin_dir();
        let mut args = vec!["build"];
        if bd.ends_with("release") {
            args.push("--release");
        }
        let status = Command::new("cargo")
            .args(args)
            .current_dir(repo.clone())
            .status()
            .unwrap();
        assert!(status.success());
        let chiseld = bd.join("chiseld").to_str().unwrap().to_string();
        env::set_var("CHISELD", chiseld);
        env::set_var("CHISEL", chisel());
        env::set_var("CHISELD_HOST", "localhost:8080");
        env::set_var("CURL", "curl -S -s -i");
        lit::run::tests(lit::event_handler::Default::default(), |config| {
            config.add_search_path("tests/lit");
            config.add_extension("lit");
            config.constants.insert("chisel".to_owned(), chisel());
            let mut path = repo.clone();
            path.push("cli/tests/test-wrapper.sh");
            config.shell = path.to_str().unwrap().to_string();
        })
        .expect("Lit tests failed");
    }
}
