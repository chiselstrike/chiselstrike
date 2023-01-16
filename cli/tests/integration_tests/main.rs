// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::common::{bin_dir, run};
use clap::Parser;
use regex::Regex;
use std::fmt::{Display, Formatter};
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::Arc;

#[path = "../common/mod.rs"]
pub mod common;

mod database;
mod framework;
mod lit;
mod rust;
mod rust_tests;
mod suite;

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Parser, Clone)]
#[command(name = "lit_test", about = "Runs integration tests")]
pub struct Opt {
    /// Regex that select tests to run.
    #[arg(short, long)]
    pub test: Option<Regex>,
    /// Database system to test with. Supported values: `sqlite` (default) and `postgres`.
    #[arg(long, default_value = "sqlite")]
    pub database: DatabaseKind,
    /// Database host name. Default: `localhost`.
    #[arg(long, default_value = "localhost")]
    pub database_host: String,
    /// Database username.
    #[arg(long)]
    pub database_user: Option<String>,
    /// Database password.
    #[arg(long)]
    pub database_password: Option<String>,
    /// Kafka connection.
    #[arg(long)]
    pub kafka_connection: Option<String>,
    #[arg(long)]
    pub optimize: Option<bool>,
    /// Number of Rust tests to run in parallel (does not apply to lit).
    #[arg(short, long)]
    pub parallel: Option<usize>,
    test_arg: Option<Regex>,

    /// don't capture stdout/stderr of each task, allow printing directly
    #[arg(long)]
    nocapture: bool,
}

fn main() -> ExitCode {
    // install the current packages in our package.json. This will make things like esbuild
    // generally available. Tests that want a specific extra package can then install on top
    run("npm", ["install"]);

    let bd = bin_dir();
    let mut args = vec!["build"];
    if bd.ends_with("release") {
        args.push("--release");
    }
    run("cargo", args);

    // prepare an empty "cache" project, where we install the necessary dependencies once, so that
    // we can reuse them in test cases
    run(
        "node",
        [
            "packages/create-chiselstrike-app/dist/index.js",
            "--chisel-api-version",
            "../../../../packages/chiselstrike-api",
            "--chisel-cli-version",
            "latest",
            "--rewrite",
            "cli/tests/integration_tests/cache",
        ],
    );

    let opt = Arc::new(Opt::parse());

    let ok = rust::run_tests(opt.clone()) & lit::run_tests(&opt);
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
