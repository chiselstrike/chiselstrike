use file_mode::ModePath;
use serde_json::json;
use std::time::Duration;

use crate::framework::prelude::*;
use crate::framework::Chisel;

async fn setup_secret_endpoint(chisel: &Chisel) {
    chisel.write_unindent(
        "routes/secret.ts",
        r##"
        import { getSecret } from "@chiselstrike/api"

        export default async function chisel(req: Request) {
            let secret = getSecret("secret");
            if (secret !== undefined) {
                return new Response(JSON.stringify(secret));
            } else {
                return new Response(JSON.stringify("no secret"));
            }
        }"##,
    );
    chisel.apply().await.unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn read_secret(mut c: TestContext) {
    setup_secret_endpoint(&c.chisel).await;

    c.chisel.write(".env", r##"{"secret": null}"##);
    c.restart_chiseld().await;
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!(null));

    c.chisel.write(".env", r##"{"secret": "string"}"##);
    c.restart_chiseld().await;
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!("string"));

    c.chisel.write(".env", r##"{"secret": true}"##);
    c.restart_chiseld().await;
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!(true));

    c.chisel.write(".env", r##"{"secret": 42}"##);
    c.restart_chiseld().await;
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!(42));

    c.chisel.write(".env", r##"{"secret": {"key": "value"}}"##);
    c.restart_chiseld().await;
    assert_eq!(
        c.chisel.get_json("/dev/secret").await,
        json!({"key": "value"})
    );

    // Read malformed secret
    c.chisel.write(".env", r##"{"secret": }"##);
    c.restart_chiseld().await;
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!("no secret"));
}

#[chisel_macros::test(modules = Deno)]
pub async fn invalid_permissions(mut c: TestContext) {
    setup_secret_endpoint(&c.chisel).await;

    c.chisel.write(".env", r##"{"secret": "Hello Dave"}"##);
    c.restart_chiseld().await;
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!("Hello Dave"));

    let env_path = c.chisel.tmp_dir.path().join(".env");
    env_path.set_mode(0o200).unwrap();
    c.chisel.write(".env", r##"{"secret": "HAL is evil"}"##);
    c.restart_chiseld().await;

    // Verify that the secret wasn't updated.
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!("no secret"));
}

#[chisel_macros::test(modules = Deno)]
pub async fn periodic_reread(mut c: TestContext) {
    setup_secret_endpoint(&c.chisel).await;

    c.chisel.write(".env", r##"{"secret": "Hello Dave"}"##);
    c.restart_chiseld().await;
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!("Hello Dave"));

    c.chisel.write(".env", r##"{"secret": "HAL is evil"}"##);
    // Sleep to test the periodic reread
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!("HAL is evil"));

    // Test that we clear secrets on reread
    c.chisel.write(".env", r##"{}"##);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    // Verify that the secret wasn't updated.
    assert_eq!(c.chisel.get_json("/dev/secret").await, json!("no secret"));
}

#[chisel_macros::test(modules = Deno)]
pub async fn load_secret_out_of_function(mut c: TestContext) {
    c.chisel.write_unindent(
        "routes/secret.ts",
        r##"
        import { getSecret } from "@chiselstrike/api"

        const issue728 = getSecret("secret");
        function foo() {
            return issue728;
        }

        export default async function chisel(req: Request) {
            return foo();
        }"##,
    );
    c.chisel.apply().await.unwrap();

    c.chisel.write(".env", r##"{"secret": "728 is fixed"}"##);
    c.restart_chiseld().await;
    assert_eq!(c.chisel.get_text("/dev/secret").await, "728 is fixed");
}
