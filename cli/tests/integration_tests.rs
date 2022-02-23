// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

use crate::common::bin_dir;
use crate::common::repo_dir;
use crate::common::run;
use std::env;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::str::FromStr;
use structopt::StructOpt;

mod common;

#[derive(Debug)]
enum Database {
    Postgres,
    Sqlite,
}

impl Display for Database {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            Database::Postgres => write!(f, "postgres"),
            Database::Sqlite => write!(f, "sqlite"),
        }
    }
}

type ParseError = &'static str;

impl FromStr for Database {
    type Err = ParseError;

    fn from_str(database: &str) -> Result<Self, Self::Err> {
        match database {
            "postgres" => Ok(Database::Postgres),
            "sqlite" => Ok(Database::Sqlite),
            _ => Err("Unsupported database"),
        }
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "lit_test", about = "Runs integration tests")]
struct Opt {
    /// Name of a signle lit test to run (e.g. `populate.lit`)
    #[structopt(short, long)]
    test: Option<String>,
    /// Database system to test with. Supported values: `sqlite` (default) and `postgres`.
    #[structopt(short, long, default_value = "sqlite")]
    database: Database,
}

fn chisel() -> String {
    bin_dir().join("chisel").to_str().unwrap().to_string()
}

fn main() {
    // install the current packages in our package.json. This will make things like esbuild
    // generally available. Tests that want a specific extra package can then install on top
    run("npm", ["install"]);

    let opt = Opt::from_args();

    let repo = repo_dir();
    let bd = bin_dir();
    let mut args = vec!["build"];
    if bd.ends_with("release") {
        args.push("--release");
    }
    run("cargo", args);

    let chiseld = bd.join("chiseld").to_str().unwrap().to_string();

    let create_app = repo
        .join("packages/create-chiselstrike-app")
        .to_str()
        .unwrap()
        .to_string();

    env::set_var("CHISELD", chiseld);
    env::set_var("CHISEL", chisel());
    env::set_var("RMCOLOR", "sed s/\x1B\\[[0-9;]*[A-Za-z]//g");
    env::set_var("CHISELD_HOST", "localhost:8080");
    env::set_var("CHISELD_LOCALHOST", "localhost:9090");
    env::set_var("CURL", "curl -S -s -i -w '\\n'");
    env::set_var("CREATE_APP", create_app);
    env::set_var("TEST_DATABASE", opt.database.to_string());

    let search_path = Path::new("tests/lit")
        .join(opt.test.unwrap_or_else(|| "".to_string()))
        .to_str()
        .unwrap()
        .to_owned();

    lit::run::tests(lit::event_handler::Default::default(), |config| {
        config.add_search_path(search_path.to_owned());
        config.add_extension("deno");
        config.add_extension("node");
        config.constants.insert("chisel".to_owned(), chisel());
        config.truncate_output_context_to_number_of_lines = Some(80);
        let mut path = repo.clone();
        path.push("cli/tests/test-wrapper.sh");
        config.shell = path.to_str().unwrap().to_string();
    })
    .expect("Lit tests failed");
}
