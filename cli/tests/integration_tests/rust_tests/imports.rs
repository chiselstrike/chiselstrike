// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[chisel_macros::test(modules = Node)]
pub async fn http(c: TestContext) {
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

#[chisel_macros::test(modules = Deno)]
pub async fn deno_land_module(mut c: TestContext) {
    c.chisel.write(
        "routes/indented.ts",
        r##"
        import indent from 'https://deno.land/x/text_indent@v0.1.0/mod.ts';

        export default async function chisel(req: Request) {
            return "test" + indent("foo", 4);
        }
    "##,
    );

    c.chisel.apply().await.unwrap();
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
