use deno_core::JsRuntime;

pub struct DenoService {
    pub runtime: JsRuntime,
}

impl DenoService {
    pub fn new() -> Self {
        Self {
            runtime: JsRuntime::new(Default::default()),
        }
    }
}
