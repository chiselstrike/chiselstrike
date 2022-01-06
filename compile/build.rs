use deno_core::JsRuntime;
use deno_core::RuntimeOptions;
use std::env;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let snapshot_path = out.join("SNAPSHOT.bin");

    let mut runtime = JsRuntime::new(RuntimeOptions {
        will_snapshot: true,
        ..Default::default()
    });

    runtime.sync_ops_cache();

    for p in ["src/typescript.js", "src/tsc.js"] {
        println!("cargo:rerun-if-changed={}", p);
        let code = std::fs::read_to_string(p).unwrap();
        runtime.execute_script(p, &code).unwrap();
    }

    let snapshot = runtime.snapshot();
    std::fs::write(&snapshot_path, snapshot).unwrap();
}
