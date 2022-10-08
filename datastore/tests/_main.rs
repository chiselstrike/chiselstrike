use anyhow::{Result, Context};
use chisel_datastore::{conn, layout};
use guard::guard;
use sqlx::prelude::*;
use std::{env, fs};
use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .context("missing cargo env CARGO_MANIFEST_DIR")?;
    let tests_dir = Path::new(&manifest_dir).join("tests");
    let test_paths = read_test_paths(&tests_dir)
        .context("could not read test files from the tests dir")?;

    for test_path in test_paths.iter() {
        println!("{}", test_path.display());
        run_test(&test_path).await?;
    }
    Ok(())
}

fn read_test_paths(tests_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut test_paths = Vec::new();
    for entry in fs::read_dir(&tests_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() { continue }
        let path = entry.path();
        if path.extension() != Some(OsStr::new("js")) { continue }
        guard!{let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue }};
        if name.starts_with("_") { continue }
        test_paths.push(path);
    }
    test_paths.sort_unstable();
    Ok(test_paths)
}

async fn run_test(test_path: &Path) -> Result<()> {
    let test_source = fs::read_to_string(test_path)
        .context("could not read test file")?;
    let test_name = test_path.display().to_string();

    let mut opts = deno_core::RuntimeOptions::default();
    opts.extensions.push(chisel_datastore::ops::extension());
    opts.extensions.push(test_extension());

    let mut js_runtime = deno_core::JsRuntime::new(opts);
    let promise = js_runtime.execute_script(&test_name, &test_source)
        .context("could not execute test script")?;
    js_runtime.resolve_value(promise).await
        .context("could not resolve test promise")?;
    js_runtime.run_event_loop(true).await
        .context("could not run event loop to completion")?;

    println!("OK");
    Ok(())
}

fn test_extension() -> deno_core::Extension {
    deno_core::ExtensionBuilder::default()
        .ops(vec![
            op_test_connect::decl(),
            op_test_execute_sql::decl(),
            op_test_println::decl(),
        ])
        .build()
}

#[deno_core::op]
async fn op_test_connect(
    op_state: Rc<RefCell<deno_core::OpState>>,
    layout: layout::Layout,
) -> Result<deno_core::ResourceId> {
    let pool = sqlx::AnyPool::connect("sqlite::memory:").await?;
    let conn = conn::DataConn::new(Arc::new(layout), pool);
    Ok(op_state.borrow_mut().resource_table.add(conn))
}

#[deno_core::op]
async fn op_test_execute_sql(
    op_state: Rc<RefCell<deno_core::OpState>>,
    conn_rid: deno_core::ResourceId,
    sql_text: String,
) -> Result<()> {
    let conn = op_state.borrow().resource_table.get::<conn::DataConn>(conn_rid)?;
    conn.pool.execute(sql_text.as_str()).await?;
    Ok(())
}

#[deno_core::op]
fn op_test_println(json: serde_json::Value) {
    if let serde_json::Value::String(value) = json {
        println!("{}", value)
    } else {
        println!("{}", serde_json::to_string_pretty(&json).unwrap())
    }
}
