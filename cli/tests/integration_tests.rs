// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

use crate::common::bin_dir;
use crate::common::repo_dir;
use crate::common::run;
use std::env;

use std::path::Path;
use structopt::StructOpt;

mod common;

#[derive(Debug, StructOpt)]
#[structopt(name = "lit_test", about = "Runs integration tests")]
struct Opt {
    /// Name of a signle lit test to run (e.g. `populate.lit`)
    #[structopt(short, long)]
    test: Option<String>,
}

fn chisel() -> String {
    bin_dir().join("chisel").to_str().unwrap().to_string()
}

fn main() {
    let opt = Opt::from_args();

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
    env::set_var("RMCOLOR", "sed s/\x1B\\[[0-9;]*[A-Za-z]//g");
    env::set_var("CHISELD_HOST", "localhost:8080");
    env::set_var("CHISELD_LOCALHOST", "localhost:9090");
    env::set_var("CURL", "curl -S -s -i -w '\\n'");

    let search_path = Path::new("tests/lit")
        .join(opt.test.unwrap_or_else(|| "".to_string()))
        .to_str()
        .unwrap()
        .to_owned();

    lit::run::tests(lit::event_handler::Default::default(), |config| {
        config.add_search_path(search_path.to_owned());
        config.add_extension("lit");
        config.constants.insert("chisel".to_owned(), chisel());
        config.truncate_output_context_to_number_of_lines = Some(80);
        let mut path = repo.clone();
        path.push("cli/tests/test-wrapper.sh");
        config.shell = path.to_str().unwrap().to_string();
    })
    .expect("Lit tests failed");
}
