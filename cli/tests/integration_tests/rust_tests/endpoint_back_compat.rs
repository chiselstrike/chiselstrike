// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[self::test(modules = Deno, optimize = Yes)]
pub async fn test_deno(c: TestContext) {
    test_modules(c, "deno").await
}

#[self::test(modules = Node, optimize = Yes)]
pub async fn test_node(c: TestContext) {
    test_modules(c, "node").await
}

async fn test_modules(c: TestContext, modules_str: &str) {
    c.chisel.write(
        "endpoints/hello.ts",
        r##"
        import { ChiselRequest } from "@chiselstrike/api";
        export default async function(req: ChiselRequest) {
            return ["hello"];
        }
        "##,
    );

    let chisel_toml = format!(
        r##"
        models = ["models"]
        endpoints = ["endpoints"]
        events = ["events"]
        policies = ["policies"]
        modules = "{}""##,
        modules_str
    );
    c.chisel.write_unindent("Chisel.toml", &chisel_toml);

    c.chisel.apply_ok().await;

    assert_eq!(c.chisel.get_json("/dev/hello").await, json!(["hello"]));
}
