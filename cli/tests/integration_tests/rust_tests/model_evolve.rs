use serde_json::json;

use crate::framework::prelude::*;
use crate::framework::{json_is_subset, Chisel};

fn write_crud_route(chisel: &Chisel) {
    chisel.write(
        "routes/evolving.ts",
        r##"
        import { Evolving } from "../models/model.ts";
        export default Evolving.crud();
        "##,
    );
}

#[chisel_macros::test(modules = Deno)]
pub async fn add_field(mut c: TestContext) {
    write_crud_route(&c.chisel);
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
        }"##,
    );
    c.chisel.apply_ok().await;

    c.chisel.post_json_ok("/dev/evolving", json!({})).await;

    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            a: string = "with_default";
        }"##,
    );
    c.chisel.apply_ok().await;

    json_is_subset(
        c.chisel.get_json("/dev/evolving").await,
        json!({
            "results": [{"a": "with_default"}],
        }),
    )
    .unwrap();

    // Ensure that changes are persisted
    c.restart_chiseld().await;
    json_is_subset(
        c.chisel.get_json("/dev/evolving").await,
        json!({
            "results": [{"a": "with_default"}],
        }),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn remove_field(c: TestContext) {
    write_crud_route(&c.chisel);
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            a: string = "";
            b: string = "";

        }"##,
    );
    c.chisel.apply_ok().await;

    c.chisel
        .post_json_ok("/dev/evolving", json!({"a": "Heracles", "b": "is cool"}))
        .await;

    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            a: string = "";
        }"##,
    );
    c.chisel.apply_ok().await;

    let r = c.chisel.get_json("/dev/evolving").await;
    json_is_subset(
        &r,
        json!({
            "results": [{"a": "Heracles"}],
        }),
    )
    .unwrap();
    let results = &r["results"].as_array().unwrap();
    assert!(results.len() == 1);
    assert!(!results[0].as_object().unwrap().contains_key("b"));
}

#[chisel_macros::test(modules = Deno)]
pub async fn change_default(c: TestContext) {
    write_crud_route(&c.chisel);
    // First declare and store entity with no fields.
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
        }"##,
    );
    c.chisel.apply_ok().await;
    c.chisel.post_json_ok("/dev/evolving", json!({})).await;

    // Then update it so that there is a defaulted field
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            a: boolean = true;
        }"##,
    );
    c.chisel.apply_ok().await;

    let r = c.chisel.get_json("/dev/evolving").await;
    json_is_subset(
        &r,
        json!({
            "results": [{"a": true}],
        }),
    )
    .unwrap();

    // And finally change the default
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            a: boolean = false;
        }"##,
    );
    c.chisel.apply_ok().await;

    let r = c.chisel.get_json("/dev/evolving").await;
    json_is_subset(
        &r,
        json!({
            "results": [{"a": false}],
        }),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn add_field_with_complex_default(c: TestContext) {
    write_crud_route(&c.chisel);
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
        }"##,
    );
    c.chisel.apply_ok().await;
    c.chisel.post_json_ok("/dev/evolving", json!({})).await;

    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            a: string = JSON.stringify({a: "test2"});
        }"##,
    );
    let mut output = c.chisel.apply_err().await;
    output.stderr.read("Error: unsafe to replace type");

    // It's ok if the field is optional.
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            a?: string = JSON.stringify({a: "test2"});
        }"##,
    );
    c.chisel.apply_ok().await;
}

#[chisel_macros::test(modules = Deno)]
pub async fn add_and_remove_optional_field(c: TestContext) {
    write_crud_route(&c.chisel);
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
        }"##,
    );
    c.chisel.apply_ok().await;
    c.chisel.post_json_ok("/dev/evolving", json!({})).await;

    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            opt_field?: boolean;
        }"##,
    );
    c.chisel.apply_ok().await;

    let r = c.chisel.get_json("/dev/evolving").await;
    let e = &r["results"].as_array().unwrap()[0].as_object().unwrap();
    assert!(!e.contains_key("opt_field"));

    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
        }"##,
    );
    c.chisel.apply_ok().await;
}

#[chisel_macros::test(modules = Deno, optimize = Yes)]
pub async fn remove_field_with_index(c: TestContext) {
    write_crud_route(&c.chisel);
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
            a: string = "";
        }"##,
    );
    c.chisel.write(
        "routes/test_indexes.ts",
        r##"
        import { Evolving } from "../models/model.ts";

        export default async function chisel(req: Request) {
            const filtered = Evolving.cursor()
                .filter((e: Evolving) => {
                    return e.a == "xx";
                });
            const results = (await filtered.toArray()).map(p => p.a);
            return new Response("[" + results.join(", ") + "]");
        }"##,
    );
    c.chisel.apply_ok().await;

    c.chisel.remove_file("routes/test_indexes.ts");
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Evolving extends ChiselEntity {
        }"##,
    );
    c.chisel.apply_ok().await;
}
