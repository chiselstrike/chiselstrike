use crate::framework::prelude::*;

fn write_crud_endpoint(chisel: &Chisel) {
    chisel.write(
        "routes/defaults.ts",
        r##"
        import { Defaulted } from "../models/default.ts";
        export default Defaulted.crud();
        "##,
    );
}

static DEFAULTED_MODEL: &str = r##"
    import { ChiselEntity } from "@chiselstrike/api";

    export class Defaulted extends ChiselEntity {
        num1: number = +1;
        num2: number = 0;
        num3: number = -1;
        bool_false: boolean = false;
        bool_true: boolean = true;
        empty_string: string = "";
        some_string: string = "some_string";
    }
"##;

lazy_static::lazy_static! {
    static ref DEFAULTED_VALUE: serde_json::Value = json!({
        "num1": 1,
        "num2": 0,
        "num3": -1,
        "bool_false": false,
        "bool_true": true,
        "empty_string": "",
        "some_string": "some_string",
    });
}

lazy_static::lazy_static! {
    static ref DEFAULTED_RESULTS: serde_json::Value = json!({
        "results": [DEFAULTED_VALUE.clone()],
    });
}

#[chisel_macros::test(modules = Deno)]
pub async fn basic(c: TestContext) {
    write_crud_endpoint(&c.chisel);
    c.chisel.write("models/default.ts", DEFAULTED_MODEL);
    c.chisel.apply_ok().await;

    c.chisel.post_json_ok("/dev/defaults", json!({})).await;
    json_is_subset(&c.chisel.get_json("dev/defaults").await, &DEFAULTED_RESULTS).unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn add_defaulted_fields(c: TestContext) {
    write_crud_endpoint(&c.chisel);
    c.chisel.write(
        "models/default.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Defaulted extends ChiselEntity {}
    "##,
    );
    c.chisel.apply_ok().await;
    c.chisel.post_json_ok("/dev/defaults", json!({})).await;

    c.chisel.write("models/default.ts", DEFAULTED_MODEL);
    c.chisel.apply_ok().await;

    json_is_subset(&c.chisel.get_json("dev/defaults").await, &DEFAULTED_RESULTS).unwrap();
}

fn assert_empty_results(value: serde_json::Value) {
    assert!(value["results"].as_array().unwrap().is_empty());
}

#[chisel_macros::test(modules = Deno)]
pub async fn filtering(c: TestContext) {
    write_crud_endpoint(&c.chisel);
    c.chisel.write("models/default.ts", DEFAULTED_MODEL);
    c.chisel.write(
        "routes/find.ts",
        r##"
        import { Defaulted } from "../models/default.ts";

        export default async function chisel(req: Request) {
            const url = new URL(req.url);
            const key = url.searchParams.get("key")!;
            const value = url.searchParams.get("value")!;
            const filter = {[key]: value} as Partial<Defaulted>;
            const results = await Defaulted.findMany(filter);
            return {results};
        }
    "##,
    );
    c.chisel.apply_ok().await;
    c.chisel.post_json_ok("/dev/defaults", json!({})).await;

    json_is_subset(
        &c.chisel
            .get_json("dev/find?key=some_string&value=some_string")
            .await,
        &DEFAULTED_RESULTS,
    )
    .unwrap();
    assert_empty_results(
        c.chisel
            .get_json("dev/find?key=empty_string&value=some")
            .await,
    );
}

#[chisel_macros::test(modules = Deno)]
pub async fn crud_filtering(c: TestContext) {
    write_crud_endpoint(&c.chisel);
    c.chisel.write("models/default.ts", DEFAULTED_MODEL);
    c.chisel.apply_ok().await;

    c.chisel.post_json_ok("/dev/defaults", json!({})).await;

    json_is_subset(
        &c.chisel.get_json("dev/defaults?.num1=1").await,
        &DEFAULTED_RESULTS,
    )
    .unwrap();
    assert_empty_results(c.chisel.get_json("dev/defaults?.num1=0").await);

    json_is_subset(
        &c.chisel.get_json("dev/defaults?.bool_false=false").await,
        &DEFAULTED_RESULTS,
    )
    .unwrap();
    assert_empty_results(c.chisel.get_json("dev/defaults?.bool_false=true").await);

    json_is_subset(
        &c.chisel
            .get_json("dev/defaults?.some_string=some_string")
            .await,
        &DEFAULTED_RESULTS,
    )
    .unwrap();
    assert_empty_results(c.chisel.get_json("dev/defaults?.some_string=some").await);
}

#[chisel_macros::test(modules = Deno)]
pub async fn crud_filtering_added_fields(c: TestContext) {
    write_crud_endpoint(&c.chisel);
    c.chisel.write(
        "models/default.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Defaulted extends ChiselEntity {}
    "##,
    );
    c.chisel.apply_ok().await;
    c.chisel.post_json_ok("/dev/defaults", json!({})).await;

    c.chisel.write("models/default.ts", DEFAULTED_MODEL);
    c.chisel.apply_ok().await;

    json_is_subset(
        &c.chisel.get_json("dev/defaults?.num1=1").await,
        &DEFAULTED_RESULTS,
    )
    .unwrap();
    assert_empty_results(c.chisel.get_json("dev/defaults?.num1=0").await);

    json_is_subset(
        &c.chisel.get_json("dev/defaults?.bool_true=true").await,
        &DEFAULTED_RESULTS,
    )
    .unwrap();
    assert_empty_results(c.chisel.get_json("dev/defaults?.bool_false=true").await);

    json_is_subset(
        &c.chisel
            .get_json("dev/defaults?.some_string=some_string")
            .await,
        &DEFAULTED_RESULTS,
    )
    .unwrap();
    assert_empty_results(c.chisel.get_json("dev/defaults?.some_string=some").await);
}

#[chisel_macros::test(modules = Deno)]
pub async fn describe(c: TestContext) {
    write_crud_endpoint(&c.chisel);
    c.chisel.write("models/default.ts", DEFAULTED_MODEL);
    c.chisel.apply_ok().await;

    c.chisel
        .describe_ok()
        .await
        .stdout
        .read("num1: number = +1;")
        .read("num2: number = 0;")
        .read("num3: number = -1;")
        .read("bool_false: boolean = false")
        .read("bool_true: boolean = true;")
        .read(r##"empty_string: string = "";"##)
        .read(r##"some_string: string = "some_string";"##);
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

    json_is_subset(
        &c.chisel.get_json("dev/defaults").await,
        &json!({"results": [{"a": 13}]}),
    )
    .unwrap();
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
    json_is_subset(
        &c.chisel.get_json("dev/defaults").await,
        &json!({
            "results": [{
                "a": "{\"a\":\"test2\"}",
                "b": "funcreturn",
                "c": {"aux": "aux_val"},
            }],
        }),
    )
    .unwrap();
}
