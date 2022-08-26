
use serde_json::json;

use crate::framework::prelude::*;
use crate::framework::{json_is_subset, Chisel};

fn write_crud_endpoint(chisel: &Chisel) {
    chisel.write(
        "routes/defaults.ts",
        r##"
        import { Defaulted } from "../models/default.ts";
        export default Defaulted.crud();
        "##,
    );
}

#[chisel_macros::test(modules = Deno)]
pub async fn non_trivial_defaults(c: TestContext) {
    write_crud_endpoint(&c.chisel);
    c.chisel.write(
        "models/default.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Defaulted extends ChiselEntity {
            a: number = "13 characters".length;
        }"##,
    );
    c.chisel.apply_ok().await;

    c.chisel.post_json_ok("/dev/defaults", json!({})).await;

    json_is_subset(c.chisel.get_json("dev/defaults").await, json!({
        "results": [{"a": 13}],
    })).unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn complex_optional_defaults(c: TestContext) {
    write_crud_endpoint(&c.chisel);
    c.chisel.write(
        "models/default.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Aux extends ChiselEntity {
            aux: string = "aux_val";
        }
        function myFunction() : string {
            return "funcreturn";
        }
        export class Defaulted extends ChiselEntity {
            a?: string = JSON.stringify({a: "test2"});
            b?: string = myFunction();
            c?: Aux = new Aux();
        }"##,
    );
    c.chisel.apply_ok().await;

    c.chisel.post_json_ok("/dev/defaults", json!({})).await;
    json_is_subset(c.chisel.get_json("dev/defaults").await, json!({
        "results": [{
            "a": "{\"a\":\"test2\"}",
            "b": "funcreturn",
            "c": {"aux": "aux_val"},
        }],
    })).unwrap();
}
