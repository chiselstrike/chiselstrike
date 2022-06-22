// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

extern crate lit;

use crate::common::bin_dir;
use crate::common::repo_dir;
use crate::common::run;
use crate::lit::event_handler::EventHandler;
use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::env;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use structopt::StructOpt;

mod common;

#[derive(Debug, Clone)]
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

#[derive(Debug, StructOpt, Clone)]
#[structopt(name = "lit_test", about = "Runs integration tests")]
struct Opt {
    /// Name of a signle lit test to run (e.g. `populate.lit`)
    #[structopt(short, long)]
    test: Option<String>,
    /// Database system to test with. Supported values: `sqlite` (default) and `postgres`.
    #[structopt(long, default_value = "sqlite")]
    database: Database,
    /// Database host name. Default: `localhost`.
    #[structopt(long, default_value = "localhost")]
    database_host: String,
    /// Database username.
    #[structopt(long)]
    database_user: Option<String>,
    /// Database password.
    #[structopt(long)]
    database_password: Option<String>,
}

fn chisel() -> String {
    bin_dir().join("chisel").to_str().unwrap().to_string()
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

    let ok_without_optimization = run_tests(opt.clone(), false);
    let ok_with_optimization = run_tests(opt, true);
    std::process::exit(if ok_with_optimization && ok_without_optimization {
        0
    } else {
        1
    });
}

fn run_tests(opt: Opt, optimize: bool) -> bool {
    if optimize {
        eprintln!("Running tests with optimization");
    } else {
        eprintln!("Running tests without optimization");
    }
    let repo = repo_dir();
    let bd = bin_dir();

    let chiseld = bd.join("chiseld").to_str().unwrap().to_string();

    let create_app = repo
        .join("packages/create-chiselstrike-app")
        .to_str()
        .unwrap()
        .to_string();

    env::set_var("CHISELD", chiseld);
    env::set_var("RMCOLOR", "sed s/\x1B\\[[0-9;]*[A-Za-z]//g");
    env::set_var("CURL", "curl -N -S -s -i -w \\n");
    env::set_var("CREATE_APP", create_app);
    env::set_var("TEST_DATABASE", opt.database.to_string());

    env::set_var("OPTIMIZE", format!("{}", optimize));

    let database_user = opt.database_user.unwrap_or_else(whoami::username);
    let mut database_url_prefix = "postgres://".to_string();
    database_url_prefix.push_str(&database_user);
    if let Some(database_password) = opt.database_password {
        database_url_prefix.push(':');
        database_url_prefix.push_str(&database_password);
    }
    database_url_prefix.push('@');
    database_url_prefix.push_str(&opt.database_host);
    env::set_var("DATABASE_URL_PREFIX", &database_url_prefix);

    let lit_files = if let Some(test_file) = opt.test {
        let path = Path::new("tests/lit").join(test_file);
        vec![std::fs::canonicalize(path).unwrap()]
    } else {
        let deno_lits = glob::glob("tests/lit/**/*.deno").unwrap();
        let node_lits = glob::glob("tests/lit/**/*.node").unwrap();
        deno_lits
            .chain(node_lits)
            .map(|path| std::fs::canonicalize(path.unwrap()).unwrap())
            .collect()
    };

    let event_handler = Arc::new(Mutex::new(lit::event_handler::Default::default()));
    let passed = Arc::new(AtomicBool::new(true));

    // ports to use for each service. Low ports will conflict with all
    // kinds of services, so go high.
    let ports = Arc::new(AtomicUsize::new(30000));
    lit_files
        .par_iter()
        .map(|test_path| -> Result<()> {
            lit::run::tests(
                GuardedEventHandler {
                    event_handler: event_handler.clone(),
                    passed: passed.clone(),
                },
                |config| {
                    // Add one to avoid conflict with local instances of chisel.
                    let rpc = ports.fetch_add(1, Ordering::Relaxed);
                    let internal = ports.fetch_add(1, Ordering::Relaxed);
                    let api = ports.fetch_add(1, Ordering::Relaxed);

                    config.test_paths = vec![test_path.clone()];
                    config.truncate_output_context_to_number_of_lines = Some(500);
                    config.always_show_stdout = true;

                    let mut path = repo.clone();
                    path.push("cli/tests/test-wrapper.sh");
                    config.shell = path.to_str().unwrap().to_string();
                    config.env_variables = HashMap::from([
                        (
                            "CHISEL".into(),
                            format!("{} --rpc-addr http://127.0.0.1:{}", chisel(), rpc),
                        ),
                        ("CHISELD_HOST".into(), format!("127.0.0.1:{}", api)),
                        ("CHISELD_INTERNAL".into(), format!("127.0.0.1:{}", internal)),
                        ("CHISELD_RPC_HOST".into(), format!("127.0.0.1:{}", rpc)),
                    ])
                },
            )
            .map_err(|_| anyhow!("'{:?}' test failed", test_path))?;
            Ok(())
        })
        .collect::<Vec<_>>();

    let mut handler = event_handler.lock().unwrap();
    handler.on_test_suite_finished(
        passed.load(Ordering::SeqCst),
        &lit::config::Config::default(),
    );
    passed.load(Ordering::SeqCst)
}

struct GuardedEventHandler {
    event_handler: Arc<Mutex<dyn lit::event_handler::EventHandler>>,
    passed: Arc<AtomicBool>,
}

impl lit::event_handler::EventHandler for GuardedEventHandler {
    fn on_test_suite_started(&mut self, _: &lit::event_handler::TestSuiteDetails, _: &lit::Config) {
    }

    fn on_test_suite_finished(&mut self, passed: bool, _config: &lit::Config) {
        self.passed.fetch_and(passed, Ordering::SeqCst);
    }

    fn on_test_finished(
        &mut self,
        mut result: lit::event_handler::TestResult,
        config: &lit::Config,
    ) {
        result.path.relative = result.path.absolute.file_name().unwrap().into();
        let mut handler = self.event_handler.lock().unwrap();
        handler.on_test_finished(result, config);
    }

    fn note_warning(&mut self, message: &str) {
        let mut handler = self.event_handler.lock().unwrap();
        handler.note_warning(message);
    }
}
