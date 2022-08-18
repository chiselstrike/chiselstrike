extern crate lit;

use crate::common::{bin_dir, get_free_port, repo_dir};
use crate::Opt;
use ::lit::event_handler::EventHandler;
use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::env;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) fn run_tests(opt: &Opt) -> bool {
    if opt.optimize.unwrap_or(true) && !run_tests_inner(opt, true) {
        return false;
    }
    if !opt.optimize.unwrap_or(false) && !run_tests_inner(opt, false) {
        return false;
    }
    true
}

fn chisel() -> String {
    bin_dir().join("chisel").to_str().unwrap().to_string()
}

fn run_tests_inner(opt: &Opt, optimize: bool) -> bool {
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

    let database_user = opt.database_user.clone().unwrap_or_else(whoami::username);
    let mut database_url_prefix = "postgres://".to_string();
    database_url_prefix.push_str(&database_user);
    if let Some(database_password) = opt.database_password.clone() {
        database_url_prefix.push(':');
        database_url_prefix.push_str(&database_password);
    }
    database_url_prefix.push('@');
    database_url_prefix.push_str(&opt.database_host);
    env::set_var("DATABASE_URL_PREFIX", &database_url_prefix);

    let deno_lits = glob::glob("tests/integration_tests/lit_tests/**/*.deno").unwrap();
    let node_lits = glob::glob("tests/integration_tests/lit_tests/**/*.node").unwrap();
    let mut lit_files = deno_lits
        .chain(node_lits)
        .map(|path| std::fs::canonicalize(path.unwrap()).unwrap())
        .collect::<Vec<_>>();

    if let Some(name_regex) = opt.test.as_ref() {
        lit_files = lit_files
            .into_iter()
            .filter(|path| name_regex.is_match(path.to_str().unwrap()))
            .collect();
    }

    if lit_files.is_empty() {
        println!("No lit files selected, skipping lit");
        return true;
    }

    let event_handler = Arc::new(Mutex::new(lit::event_handler::Default::default()));
    let passed = Arc::new(AtomicBool::new(true));

    // ports to use for each service. Low ports will conflict with all
    // kinds of services, so go high.
    let ports = Arc::new(AtomicU16::new(30000));
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
                    let rpc = get_free_port(&ports);
                    let internal = get_free_port(&ports);
                    let api = get_free_port(&ports);

                    config.test_paths = vec![test_path.clone()];
                    config.truncate_output_context_to_number_of_lines = Some(500);
                    config.always_show_stdout = false;

                    let mut path = repo.clone();
                    path.push("cli/tests/integration_tests/test-wrapper.sh");
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
