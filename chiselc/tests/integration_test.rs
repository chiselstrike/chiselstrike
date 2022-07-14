// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;

    fn chiselc() -> String {
        bin_dir().join("chiselc").to_str().unwrap().to_string()
    }

    pub fn bin_dir() -> PathBuf {
        let mut path = env::current_exe().unwrap();
        path.pop();
        path.pop();
        path
    }

    #[test]
    fn lit() {
        lit::run::tests(lit::event_handler::Default::default(), |config| {
            config.add_search_path("tests/lit");
            config.add_extension("lit");
            config.constants.insert("chiselc".to_owned(), chiselc());
            config.truncate_output_context_to_number_of_lines = Some(80);
            config.always_show_stdout = false;
        })
        .expect("Lit tests failed");
    }
}
