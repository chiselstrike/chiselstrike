// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[chisel_macros::test(modules = Node)]
pub async fn header_auth(mut c: TestContext) {
    c.chisel.apply().await.unwrap();

    // Can't access users without auth header
    c.chisel.get("/__chiselstrike/auth/users").send().await.assert_status(403);

    c.chisel.write(".env", r##"{ "CHISELD_AUTH_SECRET" : "1234" }"##);
    c.restart_chiseld().await;

    c.chisel.get("/dev/hello").send().await.assert_text("hello world");
    c.chisel.get("/__chiselstrike/auth/users").header("ChiselAuth", "1234").send().await.assert_ok();
    c.chisel.get("/__chiselstrike/auth/users").header("ChiselAuth", "12345").send().await.assert_status(403);
    c.chisel.get("/__chiselstrike/auth/users").header("ChiselAuth", "").send().await.assert_status(403);
    c.chisel.get("/__chiselstrike/auth/users").send().await.assert_status(403);
}

#[chisel_macros::test(modules = Node)]
pub async fn token_auth(mut c: TestContext) {
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        routes:
          - path: /
            mandatory_header: { name: header33, secret_value_ref: TOKEN33 }"##,
    );
    c.chisel.write(".env", r##"{ "TOKEN33" : "s3cr3t" }"##);
    c.restart_chiseld().await;
    c.chisel.apply().await.unwrap();

    // Can't access /dev/hello without the required header.
    c.chisel.get("/dev/hello").send().await.assert_status(403);

    // But with the right header, you can.
    c.chisel.get("/dev/hello").header("header33", "s3cr3t").send().await.assert_text("hello world");

    // Wrong header value.
    c.chisel.get("/dev/hello").header("header33", "wrong").send().await.assert_status(403);

    // Header spec references non-existing secret.
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        routes:
          - path: /
            mandatory_header: { name: header33, secret_value_ref: WXYZ }"##,
    );
    c.chisel.apply().await.unwrap();
    c.chisel.get("/dev/hello").header("header33", "s3cr3t").send().await.assert_status(403);

    // Repeated path for header auth.
    c.chisel.write_unindent(
        "policies/p.yaml",r##"
        routes:
          - path: /
            mandatory_header: { name: header33, secret_value_ref: TOKEN33 }
          - path: /
            mandatory_header: { name: foo, secret_value_ref: BAR }"##,
    );
    c.chisel
        .apply()
        .await
        .expect_err("Didn't catch repeat path")
        .stderr
        .peek("Repeated path in header authorization");

    // Unparsable header.
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        routes:
          - path: /
            mandatory_header: aaabbb"##,
    );
    c.chisel
        .apply()
        .await
        .expect_err("Didn't catch non-dict header")
        .stderr
        .peek("invalid type");

    // Header without name.
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        routes:
          - path: /
            mandatory_header: { secret_value_ref: TOKEN33 }"##,
    );
    c.chisel
        .apply()
        .await
        .expect_err("Didn't catch missing header name")
        .stderr
        .peek("missing field");

    // Only PUTs and GETs require a header.
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        routes:
          - path: /
            mandatory_header: { name: header33, secret_value_ref: TOKEN33, only_for_methods: [ PUT, GET ] } "##,
    );
    c.chisel.apply().await.unwrap();
    c.chisel.get("/dev/hello").send().await.assert_status(403);
    c.chisel.get("/dev/hello").header("header33", "s3cr3t").send().await.assert_text("hello world");
    c.chisel.post("/dev/hello").json(&json!(122333)).send().await.assert_text("122333");
}

#[chisel_macros::test(modules = Node)]
pub async fn endpoints_backcompat(mut c: TestContext) {
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        endpoints:
          - path: /
            mandatory_header: { name: header33, secret_value_ref: TOKEN33 }"##,
    );
    c.chisel.write(".env", r##"{ "TOKEN33" : "s3cr3t" }"##);
    c.restart_chiseld().await;
    c.chisel.apply().await.unwrap();

    // the policy is applied when we use `endpoints:` instead of `routes:`
    c.chisel.get("/dev/hello").send().await.assert_status(403);
    c.chisel.get("/dev/hello").header("header33", "s3cr3t").send().await.assert_status(200);
}
