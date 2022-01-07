use anyhow::Result;
use deno_core::op_sync;
use deno_core::JsRuntime;
use deno_core::OpState;
use deno_core::RuntimeOptions;
use std::env;
use std::path::PathBuf;

fn read(_op_state: &mut OpState, path: String, _: ()) -> Result<String> {
    if path == "bootstrap.ts" {
        return Ok("/// <reference lib=\"dom\" />".to_string());
    }
    if let Some(suffix) = path.strip_prefix("/default/lib/location/") {
        macro_rules! inc {
            ( $($e:expr),* ) => {
                match suffix {
                    $(
                        $e => include_str!(concat!("src/lib/", $e)),
                    )*
                        _ => "",
                }
            };
        }
        let content = inc!(
            "lib.dom.d.ts",
            "lib.es2015.collection.d.ts",
            "lib.es2015.core.d.ts",
            "lib.es2015.d.ts",
            "lib.es2015.generator.d.ts",
            "lib.es2015.iterable.d.ts",
            "lib.es2015.promise.d.ts",
            "lib.es2015.proxy.d.ts",
            "lib.es2015.reflect.d.ts",
            "lib.es2015.symbol.d.ts",
            "lib.es2015.symbol.wellknown.d.ts",
            "lib.es2016.array.include.d.ts",
            "lib.es2016.d.ts",
            "lib.es2017.d.ts",
            "lib.es2017.intl.d.ts",
            "lib.es2017.object.d.ts",
            "lib.es2017.sharedmemory.d.ts",
            "lib.es2017.string.d.ts",
            "lib.es2017.typedarrays.d.ts",
            "lib.es2018.asyncgenerator.d.ts",
            "lib.es2018.asynciterable.d.ts",
            "lib.es2018.d.ts",
            "lib.es2018.intl.d.ts",
            "lib.es2018.promise.d.ts",
            "lib.es2018.regexp.d.ts",
            "lib.es2019.array.d.ts",
            "lib.es2019.d.ts",
            "lib.es2019.object.d.ts",
            "lib.es2019.string.d.ts",
            "lib.es2019.symbol.d.ts",
            "lib.es2020.bigint.d.ts",
            "lib.es2020.d.ts",
            "lib.es2020.intl.d.ts",
            "lib.es2020.promise.d.ts",
            "lib.es2020.sharedmemory.d.ts",
            "lib.es2020.string.d.ts",
            "lib.es2020.symbol.wellknown.d.ts",
            "lib.es2021.d.ts",
            "lib.es2021.intl.d.ts",
            "lib.es2021.promise.d.ts",
            "lib.es2021.string.d.ts",
            "lib.es2021.weakref.d.ts",
            "lib.es5.d.ts",
            "lib.esnext.d.ts",
            "lib.esnext.intl.d.ts"
        );
        if !content.is_empty() {
            println!("cargo:rerun-if-changed=src/lib/{}", suffix);
            return Ok(content.to_string());
        }
    }
    Ok("".to_string())
}
fn write(_op_state: &mut OpState, _path: String, _content: String) -> Result<()> {
    Ok(())
}
fn get_cwd(_op_state: &mut OpState, _: (), _: ()) -> Result<String> {
    Ok("/there/is/no/cwd".to_string())
}
fn dir_exists(_op_state: &mut OpState, _path: String, _: ()) -> Result<bool> {
    Ok(false)
}
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let snapshot_path = out.join("SNAPSHOT.bin");

    let mut runtime = JsRuntime::new(RuntimeOptions {
        will_snapshot: true,
        ..Default::default()
    });

    runtime.register_op("read", op_sync(read));
    runtime.register_op("write", op_sync(write));
    runtime.register_op("get_cwd", op_sync(get_cwd));
    runtime.register_op("dir_exists", op_sync(dir_exists));
    runtime.sync_ops_cache();

    for p in ["src/typescript.js", "src/tsc.js"] {
        println!("cargo:rerun-if-changed={}", p);
        let code = std::fs::read_to_string(p).unwrap();
        runtime.execute_script(p, &code).unwrap();
    }

    let snapshot = runtime.snapshot();
    std::fs::write(&snapshot_path, snapshot).unwrap();
}
