// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;
    use std::process::Command;

    fn bin_dir() -> PathBuf {
        let mut path = env::current_exe().unwrap();
        path.pop();
        path.pop();
        path
    }

    fn repo_dir() -> PathBuf {
        let mut path = bin_dir();
        path.pop();
        path.pop();
        path
    }

    fn chisel() -> String {
        bin_dir().join("chisel").to_str().unwrap().to_string()
    }

    #[test]
    fn lit() {
        let repo = repo_dir();
        let status = Command::new("cargo")
            .args(["build"])
            .current_dir(repo.clone())
            .status()
            .unwrap();
        assert!(status.success());
        let chiseld = bin_dir().join("chiseld").to_str().unwrap().to_string();
        env::set_var("CHISELD", chiseld);
        env::set_var("CHISEL", chisel());
        lit::run::tests(lit::event_handler::Default::default(), |config| {
            config.add_search_path("tests/lit");
            config.add_extension("lit");
            config.constants.insert("chisel".to_owned(), chisel());
            config
                .constants
                .insert("curl".to_owned(), "curl -S -s -i".to_owned());
            let mut path = repo.clone();
            path.push("cli/tests/test-wrapper.sh");
            config.shell = path.to_str().unwrap().to_string();
        })
        .expect("Lit tests failed");
    }
}
