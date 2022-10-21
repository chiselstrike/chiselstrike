use anyhow::{Result, Context, bail};
use chisel_datastore::{conn, layout, migrate};
use chisel_snapshot::schema;
use deno_core::v8;
use guard::guard;
use sqlx::ValueRef;
use sqlx::prelude::*;
use std::{env, fs};
use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;

#[tokio::main]
async fn main() -> ExitCode { 
    match run_tests().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("execution of tests failed: {:?}", err);
            ExitCode::FAILURE
        }
    }
}

async fn run_tests() -> Result<bool> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .context("missing cargo env CARGO_MANIFEST_DIR")?;
    let tests_dir = Path::new(&manifest_dir).join("tests");
    let test_paths = read_test_paths(&tests_dir)
        .context("could not read test files from the tests dir")?;

    let mut opts = deno_core::RuntimeOptions::default();
    opts.extensions.push(chisel_datastore::ops::extension());
    opts.extensions.push(test_extension());

    let mut js_runtime = deno_core::JsRuntime::new(opts);

    macro_rules! execute_script {
        ($filename:literal) => {
            js_runtime.execute_script($filename, include_str!($filename))
                .context(concat!("could not execute ", $filename))?;
        }
    }
    execute_script!("_utils.js");
    execute_script!("_framework.js");
    
    for test_path in test_paths.iter() {
        let test_source = fs::read_to_string(test_path)
            .context("could not read test file")?;
        let test_rel_path = test_path.strip_prefix(&tests_dir).unwrap_or(&test_path);
        let test_name = test_rel_path.display().to_string();

        let promise = js_runtime.execute_script(&test_name, &test_source)
            .with_context(|| format!("could not execute test script {}", test_name))?;
        js_runtime.resolve_value(promise).await
            .with_context(|| format!("could not resolve test promise from {}", test_name))?;
    }

    js_runtime.run_event_loop(true).await
        .context("could not run event loop to completion")?;

    fn grab_int(scope: &mut v8::HandleScope, path: &str) -> u32 {
        let number = deno_core::JsRuntime::grab_global::<v8::Number>(scope, path)
            .unwrap_or_else(|| panic!("could not grab JS global {:?}", path));
        number.value() as u32
    }

    let mut scope = js_runtime.handle_scope();
    let fail_count = grab_int(&mut scope, "failCount");
    let pass_count = grab_int(&mut scope, "passCount");

    if fail_count + pass_count == 0 {
        println!("no test cases were found");
        Ok(false)
    } else if fail_count == 0 {
        println!("all {} passed", pass_count);
        Ok(true)
    } else {
        println!("{} passed, {} failed", pass_count, fail_count);
        Ok(false)
    }
}

fn read_test_paths(tests_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut test_paths = Vec::new();
    for entry in fs::read_dir(&tests_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() { continue }
        let path = entry.path();
        if path.extension() != Some(OsStr::new("js")) { continue }
        guard!{let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue }};
        if name.starts_with('_') { continue }
        test_paths.push(path);
    }
    test_paths.sort_unstable();
    Ok(test_paths)
}

fn test_extension() -> deno_core::Extension {
    deno_core::ExtensionBuilder::default()
        .ops(vec![
            op_test_create_db::decl(),
            op_test_migrate::decl(),
            op_test_connect::decl(),
            op_test_execute_sql::decl(),
            op_test_fetch_sql::decl(),
            op_test_println::decl(),
        ])
        .build()
}

struct DbRes(sqlx::AnyPool);
impl deno_core::Resource for DbRes {}

#[deno_core::op]
async fn op_test_create_db(op_state: Rc<RefCell<deno_core::OpState>>) -> Result<deno_core::ResourceId> {
    let pool = sqlx::AnyPool::connect("sqlite::memory:").await?;
    Ok(op_state.borrow_mut().resource_table.add(DbRes(pool)))
}

#[deno_core::op]
async fn op_test_migrate(
    op_state: Rc<RefCell<deno_core::OpState>>,
    db_rid: deno_core::ResourceId,
    old_layout: layout::Layout,
    new_schema: schema::Schema,
) -> Result<layout::Layout> {
    let db = op_state.borrow().resource_table.get::<DbRes>(db_rid)?;
    let plan_opts = migrate::PlanOpts { table_prefix: "".into() };
    let mut txn = db.0.begin().await
        .context("could not begin transaction")?;
    let new_layout = migrate::migrate(&mut txn, &old_layout, Arc::new(new_schema), &plan_opts).await?;
    txn.commit().await
        .context("could not commit transaction")?;
    Ok(new_layout)
}

#[deno_core::op]
fn op_test_connect(
    op_state: &mut deno_core::OpState,
    db_rid: deno_core::ResourceId,
    layout: layout::Layout,
) -> Result<deno_core::ResourceId> {
    let db = op_state.resource_table.get::<DbRes>(db_rid)?;
    let conn = conn::DataConn::new(Arc::new(layout), db.0.clone());
    Ok(op_state.resource_table.add(conn))
}

#[deno_core::op]
async fn op_test_execute_sql(
    op_state: Rc<RefCell<deno_core::OpState>>,
    db_rid: deno_core::ResourceId,
    sql_text: String,
) -> Result<()> {
    let db = op_state.borrow().resource_table.get::<DbRes>(db_rid)?;
    db.0.execute(sql_text.as_str()).await
        .context("could not execute the SQL statement")?;
    Ok(())
}

#[deno_core::op]
async fn op_test_fetch_sql(
    op_state: Rc<RefCell<deno_core::OpState>>,
    db_rid: deno_core::ResourceId,
    sql_text: String,
) -> Result<Vec<Vec<serde_json::Value>>> {
    let db = op_state.borrow().resource_table.get::<DbRes>(db_rid)?;
    let rows = db.0.fetch_all(sql_text.as_str()).await
        .context("could not execute the SQL statement")?;
    rows.into_iter().map(|row| {
        (0..row.len()).map(|column_i| {
            sql_column_to_json(&row, column_i)
        }).collect::<Result<Vec<_>>>()
    }).collect()
}

fn sql_column_to_json(row: &sqlx::any::AnyRow, column_i: usize) -> Result<serde_json::Value> {
    if row.try_get_raw(column_i).unwrap().is_null() {
        return Ok(serde_json::Value::Null)
    } else if let Ok(x) = row.try_get::<String, _>(column_i) {
        return Ok(serde_json::Value::String(x));
    }
    bail!("could not decode value from column {} to JSON", column_i)
}

#[deno_core::op]
fn op_test_println(json: serde_json::Value) {
    if let serde_json::Value::String(value) = json {
        println!("{}", value)
    } else {
        println!("{}", serde_json::to_string_pretty(&json).unwrap())
    }
}
