// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>
use crate::framework::prelude::*;

#[self::test(modules = Node)]
async fn http(c: TestContext) {
    c.chisel.write(
        "routes/error.ts",
        r##"
        import { foo } from "https://foo.bar";

        export default async function chisel(req: Request) {
            return foo;
        }
    "##,
    );

    let mut output = c.chisel.apply_err().await;
    output
        .stderr
        .read("Could not apply the provided code")
        .read("chiseld cannot load module https://foo.bar/ at runtime");
}

#[self::test(modules = Deno)]
async fn deno_land_module(mut c: TestContext) {
    c.chisel.write(
        "routes/indented.ts",
        r##"
        import indent from 'https://deno.land/x/text_indent@v0.1.0/mod.ts';

        export default async function chisel(req: Request) {
            return "test" + indent("foo", 4);
        }
    "##,
    );

    c.chisel.apply_ok().await;
    assert_eq!(
        c.chisel.get_text("/dev/indented").await,
        "test    foo"
    );

    // Try after restart as well
    c.restart_chiseld().await;
    assert_eq!(
        c.chisel.get_text("/dev/indented").await,
        "test    foo"
    );
}

#[self::test(modules = Node)]
async fn builtin_deno_std(c: TestContext) {
    c.chisel.write(
        "routes/semver.ts",
        r##"
        import * as semver from 'chisel://deno-std/semver/mod.ts';
        export default async function () {
            return semver.gt("1.13.1", "1.9.9");
        }"##,
    );

    c.chisel.write(
        "routes/node_buffer.ts",
        r##"
        import { Buffer } from 'chisel://deno-std/node/buffer.ts';
        export default async function () {
            return Buffer.alloc(100).byteLength;
        }"##,
    );

    c.chisel.apply_ok().await;
    c.chisel.get("/dev/semver").send().await
        .assert_json(json!(true));
    c.chisel.get("/dev/node_buffer").send().await
        .assert_json(json!(100));
}

#[self::test(modules = Node)]
async fn node(c: TestContext) {
    c.chisel.write(
        "routes/import.ts",
        r##"
        import { Buffer } from 'buffer';
        import * as assert, { AssertionError } from 'assert';
        import { StringDecoder } from 'string_decoder';

        export default async function () {
            let assertFailed = false;
            try {
                assert.equal(1, 2);
            } catch (e) {
                assertFailed = e instanceof AssertionError;
            }

            const decoder = new StringDecoder('utf-8');
            const buf = Buffer.from("žluťoučký kůň úpěl ďábelské ódy");
            const str = decoder.end(buf);

            return [assertFailed, buf.byteLength, str.length];
        }"##,
    );

    c.chisel.apply_ok().await;
    c.chisel.get("/dev/import").send().await
        .assert_json(json!([true, 43, 31]));
}

#[self::test(modules = Node)]
async fn node_dynamic_import(c: TestContext) {
    c.chisel.write(
        "routes/import.ts",
        r##"
        const buffer = import("buffer");

        export default async function () {
            return buffer.Buffer.alloc(100).byteLength;
        }"##,
    );

    c.chisel.apply_ok().await;
    c.chisel.get("/dev/import").send().await
        .assert_json(json!(100));
}

#[self::test(modules = Node)]
async fn node_require(c: TestContext) {
    c.chisel.write(
        "routes/import.ts",
        r##"
        // @ts-expect-error
        const buffer = require("buffer");

        export default async function () {
            return buffer.Buffer.alloc(100).byteLength;
        }"##,
    );

    c.chisel.apply_ok().await;
    c.chisel.get("/dev/import").send().await
        .assert_json(json!(100));
}
