// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use deno_core::anyhow;
use deno_core::op;
use deno_core::Extension;
use deno_core::JsRuntime;
use deno_core::OpState;
use deno_core::RuntimeOptions;
use std::env;
use std::path::PathBuf;

#[op]
fn read(_op_state: &mut OpState, path: String, _: ()) -> Result<String> {
    if path == "bootstrap.ts" {
        return Ok("/// <reference lib=\"deno.core\" />
                   /// <reference lib=\"deno.unstable\" />
                  export {};"
            .to_string());
    }
    if let Some(suffix) = path.strip_prefix("/default/lib/location/") {
        macro_rules! inc_and_rerun {
            ( $e:expr ) => {{
                println!("cargo:rerun-if-changed={}", $e);
                include_str!($e)
            }};
        }
        macro_rules! inc {
            ( $($e:expr),* ) => {
                match suffix {
                    $(
                        $e => inc_and_rerun!(concat!("../third_party/deno/cli/dts/", $e)),
                    )*
                        _ => "",
                }
            };
        }

        let content = match suffix {
            "lib.deno_broadcast_channel.d.ts" => inc_and_rerun!(
                "../third_party/deno/ext/broadcast_channel/lib.deno_broadcast_channel.d.ts"
            ),
            "lib.deno_console.d.ts" => {
                inc_and_rerun!("../third_party/deno/ext/console/lib.deno_console.d.ts")
            }
            "lib.deno_core.d.ts" => inc_and_rerun!("../third_party/deno/core/lib.deno_core.d.ts"),
            "lib.deno_crypto.d.ts" => {
                inc_and_rerun!("../third_party/deno/ext/crypto/lib.deno_crypto.d.ts")
            }
            "lib.deno_fetch.d.ts" => {
                inc_and_rerun!("../third_party/deno/ext/fetch/lib.deno_fetch.d.ts")
            }
            "lib.deno_net.d.ts" => inc_and_rerun!("../third_party/deno/ext/net/lib.deno_net.d.ts"),
            "lib.deno_url.d.ts" => inc_and_rerun!("../third_party/deno/ext/url/lib.deno_url.d.ts"),
            "lib.deno_web.d.ts" => inc_and_rerun!("../third_party/deno/ext/web/lib.deno_web.d.ts"),
            "lib.deno_webgpu.d.ts" => {
                inc_and_rerun!("../third_party/deno/cli/dts/lib.deno_webgpu.d.ts")
            }
            "lib.deno_websocket.d.ts" => {
                inc_and_rerun!("../third_party/deno/ext/websocket/lib.deno_websocket.d.ts")
            }
            "lib.deno_webstorage.d.ts" => {
                inc_and_rerun!("../third_party/deno/ext/webstorage/lib.deno_webstorage.d.ts")
            }
            _ => inc!(
                "lib.deno.ns.d.ts",
                "lib.deno.shared_globals.d.ts",
                "lib.deno.unstable.d.ts",
                "lib.deno.window.d.ts",
                "lib.dom.asynciterable.d.ts",
                "lib.dom.d.ts",
                "lib.dom.iterable.d.ts",
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
                "lib.es2022.array.d.ts",
                "lib.es2022.d.ts",
                "lib.es2022.error.d.ts",
                "lib.es2022.object.d.ts",
                "lib.es2022.string.d.ts",
                "lib.es5.d.ts",
                "lib.esnext.array.d.ts",
                "lib.esnext.d.ts",
                "lib.esnext.intl.d.ts"
            ),
        };
        if !content.is_empty() {
            return Ok(content.to_string());
        }
    }
    panic!("Unexpected file at build time: {}", path);
}
#[op]
fn write(_op_state: &mut OpState, _path: String, _content: String) -> Result<()> {
    Ok(())
}
#[op]
fn get_cwd(_op_state: &mut OpState, _: (), _: ()) -> Result<String> {
    Ok("/there/is/no/cwd".to_string())
}
#[op]
fn dir_exists(_op_state: &mut OpState, _path: String, _: ()) -> Result<bool> {
    Ok(false)
}
#[op]
fn diagnostic(_op_state: &mut OpState, msg: String, _: ()) -> Result<()> {
    panic!("unexpected: {}", msg);
}
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let snapshot_path = out.join("SNAPSHOT.bin");

    let ext = Extension::builder()
        .ops(vec![
            diagnostic::decl(),
            read::decl(),
            write::decl(),
            get_cwd::decl(),
            dir_exists::decl(),
        ])
        .build();

    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![ext],
        will_snapshot: true,
        ..Default::default()
    });

    for p in ["../third_party/deno/cli/tsc/00_typescript.js", "src/tsc.js"] {
        println!("cargo:rerun-if-changed={}", p);
        let code = std::fs::read_to_string(p).unwrap();
        runtime.execute_script(p, &code).unwrap();
    }

    let snapshot = runtime.snapshot();
    std::fs::write(&snapshot_path, snapshot).unwrap();
}
