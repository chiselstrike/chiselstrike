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
            use chisel_server::server::Opt;
            use structopt::StructOpt;
            let rt = tokio::runtime::Runtime::new().unwrap();
            let opt = Opt::from_iter(
                vec!["", "-d", "sqlite://:memory:", "-m", "sqlite://:memory:"].iter(),
            );
            let server = server::run_on_new_localset(opt);
            rt.block_on(server).unwrap();
        });
        thread::sleep(time::Duration::from_secs(1));
        lit::run::tests(lit::event_handler::Default::default(), |config| {
            config.add_search_path("tests/lit");
            config.add_extension("lit");
            config.constants.insert("chisel".to_owned(), chisel());
            config
                .constants
                .insert("curl".to_owned(), "curl -S -s -i".to_owned());
        })
        .expect("Lit tests failed");
    }
}
