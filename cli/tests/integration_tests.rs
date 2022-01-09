// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

mod common;

#[cfg(test)]
mod tests {
    use crate::common::bin_dir;
    use crate::common::repo_dir;
    use crate::common::run;
    use ntest::timeout;
    use std::env;

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
        run("cargo", args);
        let chiseld = bd.join("chiseld").to_str().unwrap().to_string();
        env::set_var("CHISELD", chiseld);
        env::set_var("CHISEL", chisel());
        env::set_var("CHISEL_TSC", "/bin/true");
        env::set_var("CHISELD_HOST", "localhost:8080");
        env::set_var("CHISELD_LOCALHOST", "localhost:9090");
        env::set_var("CURL", "curl -S -s -i -w '\\n'");
        lit::run::tests(lit::event_handler::Default::default(), |config| {
            config.add_search_path("tests/lit");
            config.add_extension("lit");
            config.constants.insert("chisel".to_owned(), chisel());
            config.truncate_output_context_to_number_of_lines = Some(80);
            let mut path = repo.clone();
            path.push("cli/tests/test-wrapper.sh");
            config.shell = path.to_str().unwrap().to_string();
        })
        .expect("Lit tests failed");
    }
}
