use crate::framework::{
    ChiseldConfig, DatabaseConfig, IntegrationTest, OpMode, PostgresConfig, TestConfig,
};
use crate::rust_tests;
use crate::{Database, Opt};
use colored::Colorize;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

fn get_free_port(ports_counter: &Arc<AtomicU16>) -> u16 {
    for _ in 0..10000 {
        let port = ports_counter.fetch_add(1, Ordering::Relaxed);
        if port_scanner::local_port_available(port) {
            return port;
        }
    }
    panic!("failed to find free port in 10000 iterations");
}

fn generate_chiseld_config(ports_counter: &Arc<AtomicU16>) -> ChiseldConfig {
    let make_address = || {
        format!("127.0.0.1:{}", get_free_port(ports_counter))
            .parse()
            .unwrap()
    };
    ChiseldConfig {
        public_address: make_address(),
        rpc_address: make_address(),
        internal_address: make_address(),
    }
}

fn generate_database_config(opt: &Opt) -> DatabaseConfig {
    match opt.database {
        Database::Sqlite => DatabaseConfig::Sqlite,
        Database::Postgres => DatabaseConfig::Postgres(PostgresConfig::new(
            opt.database_host.clone(),
            opt.database_user.clone(),
            opt.database_password.clone(),
        )),
    }
}

fn generate_test_config(
    opt: &Opt,
    ports_counter: &Arc<AtomicU16>,
    mode: &OpMode,
    optimize: bool,
) -> TestConfig {
    TestConfig {
        db_config: generate_database_config(opt),
        mode: mode.clone(),
        optimize,
        chiseld_config: generate_chiseld_config(ports_counter),
    }
}

async fn run_test(
    opt: &Opt,
    ports_counter: &Arc<AtomicU16>,
    optimize: bool,
    test: &IntegrationTest,
) {
    let config = generate_test_config(opt, ports_counter, &test.mode, optimize);
    test.test_fn.call(config).await;
    println!(
        "{}(optimize={}) {}",
        test.name.green(),
        format!("{optimize}").blue(),
        "PASSED".green()
    );
}

#[tokio::main]
pub(crate) async fn run_tests(opt: &Opt) -> bool {
    let ports_counter = Arc::new(AtomicU16::new(40000));

    for test in &rust_tests::all_tests() {
        if opt.test.is_some() && test.name != opt.test.as_ref().unwrap() {
            continue;
        }

        if opt.optimize.unwrap_or(true) {
            run_test(opt, &ports_counter, true, test).await;
        }
        if !opt.optimize.unwrap_or(false) {
            run_test(opt, &ports_counter, false, test).await;
        }
    }

    true
}
