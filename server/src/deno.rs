use anyhow::Result;
use deno_core::JsRuntime;
use std::cell::RefCell;

/// A v8 isolate doesn't want to be moved between or used from
/// multiple threads. A JsRuntime owns an isolate, so we need to use a
/// thread local storage.
///
/// This has an interesting implication: We cannot easily provide a way to
/// hold transient server state, since each request can hit a different
/// thread. A client that wants that would have to put the information in
/// a database or cookie as appropriate.
///
/// The above is probably fine, since at some point we will be
/// sharding our server, so there is not going to be a single process
/// anyway.
struct DenoService {
    runtime: JsRuntime,
}

impl DenoService {
    pub fn new() -> Self {
        Self {
            runtime: JsRuntime::new(Default::default()),
        }
    }
}

thread_local! {
    static DENO: RefCell<DenoService> = RefCell::new(DenoService::new())
}

pub fn run_js(path: &str, code: &str) -> Result<String> {
    DENO.with(|d| {
        let r = &mut d.borrow_mut().runtime;
        let res = r.execute_script(path, code)?;
        let scope = &mut r.handle_scope();
        Ok(res.get(scope).to_rust_string_lossy(scope))
    })
}
