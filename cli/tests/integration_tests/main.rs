// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
use crate::common::{bin_dir, run};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use structopt::StructOpt;

#[path = "../common/mod.rs"]
pub mod common;

mod lit;

#[derive(Debug, Clone)]
pub enum Database {
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

#[derive(Debug, StructOpt, Clone)]
#[structopt(name = "lit_test", about = "Runs integration tests")]
pub(crate) struct Opt {
    /// Name of a signle lit test to run (e.g. `populate.lit`)
    #[structopt(short, long)]
    pub test: Option<String>,
    /// Database system to test with. Supported values: `sqlite` (default) and `postgres`.
    #[structopt(long, default_value = "sqlite")]
    pub database: Database,
    /// Database host name. Default: `localhost`.
    #[structopt(long, default_value = "localhost")]
    pub database_host: String,
    /// Database username.
    #[structopt(long)]
    pub database_user: Option<String>,
    /// Database password.
    #[structopt(long)]
    pub database_password: Option<String>,
    #[structopt(long)]
    pub optimize: Option<bool>,
}

fn main() {
    // install the current packages in our package.json. This will make things like esbuild
    // generally available. Tests that want a specific extra package can then install on top
    run("npm", ["install"]);

    let opt = Opt::from_args();

    let bd = bin_dir();
    let mut args = vec!["build"];
    if bd.ends_with("release") {
        args.push("--release");
    }
    run("cargo", args);

    let run_results = vec![lit::run_tests(opt)];
    std::process::exit(if run_results.iter().all(|x| *x) { 0 } else { 1 });
}
