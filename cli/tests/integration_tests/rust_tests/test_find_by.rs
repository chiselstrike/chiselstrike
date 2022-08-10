use serde_json::json;

use crate::framework::{IntegrationTest, OpMode, TestConfig};

#[chisel_macros::test(mode = OpMode::Deno)]
pub async fn test_find_by(config: TestConfig) {
    let mut ctx = config.setup().await;
    let (chisel, chiseld) = ctx.get_chisels();

    chisel.copy_to_dir("examples/person.ts", "models");
    chisel.copy_to_dir("examples/find_by.ts", "endpoints");
    chisel.copy_and_rename("examples/store.ts", "endpoints/ins.ts");

    let r = chisel.apply().expect("chisel apply failed");
    r.stdout
        .peek("Model defined: Person")
        .peek("End point defined: /dev/find_by")
        .peek("End point defined: /dev/ins");

    chisel
        .post_text(
            "/dev/ins",
            json!({
                "first_name":"Glauber",
                "last_name":"Costa",
                "age": 666,
                "human": true,
                "height": 10.01
            }),
        )
        .await;
    chisel
        .post_text(
            "/dev/ins",
            json!({
                "first_name":"Jan",
                "last_name":"Plhak",
                "age": -666,
                "human": true,
                "height": 10.02
            }),
        )
        .await;

    let resp_txt = chisel
        .post_text(
            "/dev/find_by",
            json!({
                "field_name":"first_name",
                "value":"Jan"
            }),
        )
        .await;
    assert_eq!(resp_txt, "Jan Plhak -666 true 10.02 ");

    let resp_txt = chisel
        .post_text(
            "/dev/find_by",
            json!({
                "field_name":"last_name",
                "value":"Costa"
            }),
        )
        .await;
    assert_eq!(resp_txt, "Glauber Costa 666 true 10.01 ");

    let resp_txt = chisel
        .post_text(
            "/dev/find_by",
            json!({
                "field_name":"last_name",
                "value":"bagr"
            }),
        )
        .await;
    assert_eq!(resp_txt, "");

    let resp_txt = chisel
        .post_text(
            "/dev/find_by",
            json!({
                "field_name":"age",
                "value":-666
            }),
        )
        .await;
    assert_eq!(resp_txt, "Jan Plhak -666 true 10.02 ");

    let resp_txt = chisel
        .post_text(
            "/dev/find_by",
            json!({
                "field_name":"human",
                "value":true
            }),
        )
        .await;
    assert_eq!(
        resp_txt,
        "Glauber Costa 666 true 10.01 Jan Plhak -666 true 10.02 "
    );

    let resp_txt = chisel
        .post_text(
            "/dev/find_by",
            json!({
                "field_name":"height",
                "value":10.01
            }),
        )
        .await;
    assert_eq!(resp_txt, "Glauber Costa 666 true 10.01 ");

    let resp_txt = chisel
        .post_text(
            "/dev/find_by",
            json!({
                "field_name":"height",
            }),
        )
        .await;
    assert_eq!(
        resp_txt,
        "Glauber Costa 666 true 10.01 Jan Plhak -666 true 10.02 "
    );

    let r = chisel
        .post(
            "/dev/find_by",
            json!({
                "field_name":"misspelled_field_name",
                "value":10.01
            }),
        )
        .await;
    assert!(r.is_err());

    chiseld
        .stderr
        .read("Error: expression error: entity 'Person' doesn't have field 'misspelled_field_name'")
        .await;
}
