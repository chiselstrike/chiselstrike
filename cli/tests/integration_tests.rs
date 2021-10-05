// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::{env, thread, time};

    fn bin_dir() -> PathBuf {
        let mut path = env::current_exe().unwrap();
        path.pop();
        path.pop();
        path
    }

    fn chiseld() -> String {
        bin_dir().join("chiseld").to_str().unwrap().to_string()
    }

    fn chisel() -> String {
        bin_dir().join("chisel").to_str().unwrap().to_string()
    }

    #[test]
    fn lit() {
        let mut cmd = Command::new(chiseld()).spawn().unwrap();
        // FIXME: Add a proper check that ensures server is running.
        thread::sleep(time::Duration::from_secs(1));
        lit::run::tests(lit::event_handler::Default::default(), |config| {
            config.add_search_path("tests/lit");
            config.add_extension("lit");
            config.constants.insert("chisel".to_owned(), chisel());
        })
        .expect("Lit tests failed");
        cmd.kill().unwrap();
        cmd.wait().unwrap();
        // FIXME: Kill server if tests fail.
    }
}
