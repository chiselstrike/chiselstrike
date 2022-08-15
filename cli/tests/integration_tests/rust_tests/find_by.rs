use crate::framework::prelude::*;
use serde_json::json;

pub async fn test(mut c: TestContext) {
    c.chisel.copy_to_dir("examples/person.ts", "models");
    c.chisel.copy_to_dir("examples/find_by.ts", "routes");
    c.chisel
        .copy_and_rename("examples/store.ts", "routes/ins.ts");

    let r = c.chisel.apply_ok().await;
    r.stdout.peek("Model defined: Person");

    c.chisel
        .post_json_ok(
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
    c.chisel
        .post_json_ok(
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

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"first_name",
                "value":"Jan"
            }),
        )
        .await;
    assert_eq!(resp_txt, "Jan Plhak -666 true 10.02 ");

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"last_name",
                "value":"Costa"
            }),
        )
        .await;
    assert_eq!(resp_txt, "Glauber Costa 666 true 10.01 ");

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"last_name",
                "value":"bagr"
            }),
        )
        .await;
    assert_eq!(resp_txt, "");

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"age",
                "value":-666
            }),
        )
        .await;
    assert_eq!(resp_txt, "Jan Plhak -666 true 10.02 ");

    let resp_txt = c
        .chisel
        .post_json_text(
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

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"height",
                "value":10.01
            }),
        )
        .await;
    assert_eq!(resp_txt, "Glauber Costa 666 true 10.01 ");

    let resp_txt = c
        .chisel
        .post_json_text(
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

    let resp = c
        .chisel
        .post_json(
            "/dev/find_by",
            json!({
                "field_name":"misspelled_field_name",
                "value":10.01
            }),
        )
        .await;
    assert!(resp.status().is_server_error());

    c.chiseld
        .stderr
        .read("Error: expression error: entity 'Person' doesn't have field 'misspelled_field_name'")
        .await;
}
