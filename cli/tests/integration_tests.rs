// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

#[cfg(test)]
mod tests {
    use chisel_server::server;
    use std::path::PathBuf;
    use std::{env, thread, time};

    fn bin_dir() -> PathBuf {
        let mut path = env::current_exe().unwrap();
        path.pop();
        path.pop();
        path
    }

    fn chisel() -> String {
        bin_dir().join("chisel").to_str().unwrap().to_string()
    }

    #[test]
    fn lit() {
        thread::spawn(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let server = server::run_on_new_localset();
            rt.block_on(server).unwrap();
        });
        thread::sleep(time::Duration::from_secs(1));
        lit::run::tests(lit::event_handler::Default::default(), |config| {
            config.add_search_path("tests/lit");
            config.add_extension("lit");
            config.constants.insert("chisel".to_owned(), chisel());
        })
        .expect("Lit tests failed");
    }
}
