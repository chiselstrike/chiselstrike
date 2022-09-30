use crate::framework::prelude::*;

fn write_files(chisel: &Chisel, typ: &str) {
    chisel.write(
        "models/optional.ts",
        &format!(
            r#"
            import {{ ChiselEntity }} from "@chiselstrike/api"
            export class Optional extends ChiselEntity {{
                a?: {typ};
            }}"#
        ),
    );
    chisel.write(
        "routes/store.ts",
        r#"
        import { Optional } from "../models/optional.ts";
        export default async function chisel(req: Request) {
            return await Optional.create(await req.json());
        }
        "#,
    );
    chisel.write(
        "routes/read_all.ts",
        r#"
        import { Optional } from "../models/optional.ts";
        export default async function chisel(req: Request) {
            return Optional.cursor()
                .sortBy("a")
                .map(o => {
                    // Delete ID to make json checking simpler
                    delete o.id;
                    return o;
                })
                .toArray();
        }
        "#,
    );
    chisel.write(
        "routes/optional.ts",
        r#"
        import { Optional } from "../models/optional.ts";
        export default Optional.crud();
    "#,
    );
}

#[chisel_macros::test(modules = Deno)]
pub async fn empty_and_null(c: TestContext) {
    write_files(&c.chisel, "string");
    c.chisel.apply_ok().await;

    c.chisel.post_json("/dev/store", json!({})).await;
    c.chisel
        .get("/dev/read_all")
        .send()
        .await
        .assert_json(json!([{}]));

    // Store null explicitly
    c.chisel.post_json("/dev/store", json!({ "a": null })).await;
    c.chisel
        .get("/dev/read_all")
        .send()
        .await
        .assert_json(json!([{}, {}]));
}

async fn run_primitve_test(c: TestContext, val1: serde_json::Value, val2: serde_json::Value) {
    c.chisel.apply_ok().await;

    c.chisel.post_json("/dev/store", &val1).await;
    c.chisel
        .get("/dev/read_all")
        .send()
        .await
        .assert_json(json!([&val1]));

    // Test with CRUD
    c.chisel.post_json("/dev/optional", &val2).await;
    let r = c.chisel.get_json("/dev/optional?sort=a").await;
    json_is_subset(&r, &json!({"results": [val2, val1]})).unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn number(c: TestContext) {
    write_files(&c.chisel, "number");
    run_primitve_test(c, json!({"a": 42}), json!({"a": 13})).await;
}

#[chisel_macros::test(modules = Deno)]
pub async fn string(c: TestContext) {
    write_files(&c.chisel, "string");
    run_primitve_test(c, json!({"a": "Ulala!"}), json!({"a": "Hello optional World"})).await;
}

#[chisel_macros::test(modules = Deno)]
pub async fn boolean(c: TestContext) {
    write_files(&c.chisel, "boolean");
    run_primitve_test(c, json!({"a": true}), json!({"a": false})).await;
}

#[chisel_macros::test(modules = Deno)]
pub async fn array(c: TestContext) {
    write_files(&c.chisel, "number[]");
    run_primitve_test(c, json!({"a": [42]}), json!({"a": [13]})).await;
}

#[chisel_macros::test(modules = Deno)]
pub async fn entity(c: TestContext) {
    c.chisel.write(
        "models/optional.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api"
        export class Other {
            b: string = "What is Love?"
        }
        export class Optional extends ChiselEntity {
            a?: Other;
        }"#,
    );
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { Optional } from "../models/optional.ts";
        export default async function chisel(req: Request) {
            return await Optional.create(await req.json());
        }
        "#,
    );
    c.chisel.write(
        "routes/read_all.ts",
        r#"
        import { Optional } from "../models/optional.ts";
        export default async function chisel(req: Request) {
            return Optional.cursor()
                .map(o => {
                    // Delete ID to make json checking simpler
                    delete o.id;
                    return o;
                })
                .toArray();
        }
        "#,
    );
    c.chisel.write(
        "routes/optional.ts",
        r#"
        import { Optional } from "../models/optional.ts";
        export default Optional.crud();
    "#,
    );
    c.chisel.apply_ok().await;

    // Test store/read using custom endpoint
    c.chisel.post_json("/dev/store", json!({"a": {}})).await;
    let e1 = json!({"a": {"b": "What is Love?"}});

    let r = c.chisel.get_json("/dev/read_all").await;
    let results = r.as_array().unwrap();
    assert!(results.iter().any(|e| json_is_subset(e, &e1).is_ok()));

    // Test with CRUD
    let e2 = json!({"a": {"b": "Wubba Lubba Dub Dub"}});
    c.chisel.post_json("/dev/optional", &e2).await;

    let r = c.chisel.get_json("/dev/optional").await;
    let results = r["results"].as_array().unwrap();
    assert!(results.iter().any(|e| json_is_subset(e, &e2).is_ok()));
}
