pub const JS_FILES: [(&str, &str); 2] = [
    (
        "00_typescript.js",
        include_str!("../../third_party/deno/cli/tsc/00_typescript.js"),
    ),
    ("tsc.js", include_str!("tsc.js")),
];

pub fn read(path: &str) -> &'static str {
    if path == "bootstrap.ts" {
        return "/// <reference lib=\"deno.core\" />
                  export {};";
    }
    if let Some(suffix) = path.strip_prefix("/default/lib/location/") {
        macro_rules! inc_and_rerun {
            ( $e:expr ) => {{
                include_str!($e)
            }};
        }
        macro_rules! inc {
            ( $($e:expr),* ) => {
                match suffix {
                    $(
                        $e => inc_and_rerun!(concat!("../../third_party/deno/cli/dts/", $e)),
                    )*
                        _ => "",
                }
            };
        }

        match suffix {
            "lib.deno_broadcast_channel.d.ts" => inc_and_rerun!(
                "../../third_party/deno/ext/broadcast_channel/lib.deno_broadcast_channel.d.ts"
            ),
            "lib.deno_console.d.ts" => {
                inc_and_rerun!("../../third_party/deno/ext/console/lib.deno_console.d.ts")
            }
            "lib.deno_core.d.ts" => {
                inc_and_rerun!("../../third_party/deno/core/lib.deno_core.d.ts")
            }
            "lib.deno_crypto.d.ts" => {
                inc_and_rerun!("../../third_party/deno/ext/crypto/lib.deno_crypto.d.ts")
            }
            "lib.deno_fetch.d.ts" => {
                inc_and_rerun!("../../third_party/deno/ext/fetch/lib.deno_fetch.d.ts")
            }
            "lib.deno_net.d.ts" => {
                inc_and_rerun!("../../third_party/deno/ext/net/lib.deno_net.d.ts")
            }
            "lib.deno_url.d.ts" => {
                inc_and_rerun!("../../third_party/deno/ext/url/lib.deno_url.d.ts")
            }
            "lib.deno_web.d.ts" => {
                inc_and_rerun!("../../third_party/deno/ext/web/lib.deno_web.d.ts")
            }
            "lib.deno_webgpu.d.ts" => {
                inc_and_rerun!("../../third_party/deno/cli/dts/lib.deno_webgpu.d.ts")
            }
            "lib.deno_websocket.d.ts" => {
                inc_and_rerun!("../../third_party/deno/ext/websocket/lib.deno_websocket.d.ts")
            }
            "lib.deno_webstorage.d.ts" => {
                inc_and_rerun!("../../third_party/deno/ext/webstorage/lib.deno_webstorage.d.ts")
            }
            _ => inc!(
                "lib.deno.ns.d.ts",
                "lib.deno.shared_globals.d.ts",
                "lib.deno.unstable.d.ts",
                "lib.deno.window.d.ts",
                "lib.deno.worker.d.ts",
                "lib.dom.asynciterable.d.ts",
                "lib.dom.extras.d.ts",
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
        }
    } else {
        ""
    }
}
