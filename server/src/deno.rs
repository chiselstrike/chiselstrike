use deno_core::JsRuntime;
use std::cell::RefCell;

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

pub fn run_js(code: &str) -> String {
    DENO.with(|d| {
        let r = &mut d.borrow_mut().runtime;
        // FIXME: we must propagate this error.
        let res = r.execute_script("<internal>", code).unwrap();
        let scope = &mut r.handle_scope();
        res.get(scope).to_rust_string_lossy(scope)
    })
}
