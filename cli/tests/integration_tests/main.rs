// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::common::{bin_dir, run};
use regex::Regex;
use std::fmt::{Display, Formatter};
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::Arc;
use structopt::StructOpt;

#[path = "../common/mod.rs"]
pub mod common;

mod database;
mod framework;
mod lit;
mod rust;
mod rust_tests;
mod suite;

#[derive(Debug, Clone)]
pub enum DatabaseKind {
    Postgres,
    Sqlite,
}

impl Display for DatabaseKind {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            DatabaseKind::Postgres => write!(f, "postgres"),
            DatabaseKind::Sqlite => write!(f, "sqlite"),
        }
    }
}

type ParseError = &'static str;

impl FromStr for DatabaseKind {
    type Err = ParseError;

    fn from_str(database: &str) -> Result<Self, Self::Err> {
        match database {
            "postgres" => Ok(DatabaseKind::Postgres),
            "sqlite" => Ok(DatabaseKind::Sqlite),
            _ => Err("Unsupported database"),
        }
    }
}

#[derive(Debug, StructOpt, Clone)]
#[structopt(name = "lit_test", about = "Runs integration tests")]
pub struct Opt {
    /// Regex that select tests to run.
    #[structopt(short, long)]
    pub test: Option<Regex>,
    /// Database system to test with. Supported values: `sqlite` (default) and `postgres`.
    #[structopt(long, default_value = "sqlite")]
    pub database: DatabaseKind,
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
    /// Number of Rust tests to run in parallel (does not apply to lit).
    #[structopt(short, long)]
    pub parallel: Option<usize>,
}

fn main() -> ExitCode {
    // install the current packages in our package.json. This will make things like esbuild
    // generally available. Tests that want a specific extra package can then install on top
    run("npm", ["install"]);

    let opt = Arc::new(Opt::from_args());

    let bd = bin_dir();
    let mut args = vec!["build"];
    if bd.ends_with("release") {
        args.push("--release");
    }
    run("cargo", args);

    let ok = rust::run_tests(opt.clone()) & lit::run_tests(&opt);
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
