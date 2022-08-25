use crate::framework::{header, prelude::*};
use reqwest::StatusCode;
use serde_json::json;

#[chisel_macros::test(modules = Node)]
pub async fn header_auth(mut c: TestContext) {
    c.chisel.apply().await.unwrap();

    // Can't access users without auth header
    assert_eq!(
        c.chisel.get_status("/__chiselstrike/auth/users").await,
        StatusCode::FORBIDDEN
    );

    c.chisel.write(".env", r##"{ "CHISELD_AUTH_SECRET" : "1234" }"##);
    c.restart_chiseld().await;

    assert_eq!(c.chisel.get_text("/dev/hello").await, "hello world");
    assert_eq!(
        c.chisel.get_status_with_headers("/__chiselstrike/auth/users", header("ChiselAuth", "1234")).await,
        StatusCode::OK
    );
    assert_eq!(
        c.chisel.get_status_with_headers("/__chiselstrike/auth/users", header("ChiselAuth", "12345")).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        c.chisel.get_status_with_headers("/__chiselstrike/auth/users", header("ChiselAuth", "")).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        c.chisel.get_status("/__chiselstrike/auth/users").await,
        StatusCode::FORBIDDEN
    );
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
    assert_eq!(
        c.chisel.get_status("/dev/hello").await,
        StatusCode::FORBIDDEN
    );

    // But with the right header, you can.
    assert_eq!(
        c.chisel
            .get_text_with_headers("/dev/hello", header("header33", "s3cr3t"))
            .await,
        "hello world"
    );

    // Wrong header value.
    assert_eq!(
        c.chisel
            .get_status_with_headers("/dev/hello", header("header33", "wrong"))
            .await,
        StatusCode::FORBIDDEN
    );

    // Header spec references non-existing secret.
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        routes:
          - path: /
            mandatory_header: { name: header33, secret_value_ref: WXYZ }"##,
    );
    c.chisel.apply().await.unwrap();
    assert_eq!(
        c.chisel
            .get_status_with_headers("/dev/hello", header("header33", "s3cr3t"))
            .await,
        StatusCode::FORBIDDEN
    );

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
        .expect_err("Didn't catch wrong header")
        .stderr
        .peek("Unparsable header");

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
        .expect_err("Didn't catch wrong header")
        .stderr
        .peek("Header must have string values for keys 'name' and 'secret_value_ref'");

    // Non-string secret_value_ref.
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        routes:
          - path: /
            mandatory_header: { name: header33, secret_value_ref: 99 }"##,
    );
    c.chisel
        .apply()
        .await
        .expect_err("Didn't catch wrong header")
        .stderr
        .peek("Header must have string values for keys 'name' and 'secret_value_ref'");

    // Only PUTs and GETs require a header.
    c.chisel.write_unindent(
        "policies/p.yaml", r##"
        routes:
          - path: /
            mandatory_header: { name: header33, secret_value_ref: TOKEN33, only_for_methods: [ PUT, GET ] } "##,
    );
    c.chisel.apply().await.unwrap();
    assert_eq!(
        c.chisel.get_status("/dev/hello").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        c.chisel
            .get_text_with_headers("/dev/hello", header("header33", "s3cr3t"))
            .await,
        "hello world"
    );
    assert_eq!(
        c.chisel.post_json_text("/dev/hello", json!(122333)).await,
        "122333"
    );
}
