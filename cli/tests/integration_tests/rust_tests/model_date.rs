use crate::framework::prelude::*;

#[chisel_macros::test(modules = Deno)]
pub async fn store_and_load(c: TestContext) {
    c.chisel.write(
        "models/dated.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api"
        export class Dated extends ChiselEntity {
            date: Date;
        }"#,
    );
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { Dated } from "../models/dated.ts";
        export default async function chisel(req: Request) {
            await Dated.create({
                date: Date.parse('01 Sep 2022 12:13:14 GMT'),
            });
        }
        "#,
    );
    c.chisel.write(
        "routes/read.ts",
        r#"
        import { Dated } from "../models/dated.ts";
        export default async function chisel(req: Request) {
            let dated = (await Dated.findOne({}))!;
            return dated.date.toUTCString();
        }
        "#,
    );
    c.chisel.apply_ok().await;
    c.chisel.post_json("/dev/store", json!({})).await;
    assert_eq!(
        c.chisel.get_text("/dev/read").await,
        "Thu, 01 Sep 2022 12:13:14 GMT"
    );
}

#[chisel_macros::test(modules = Deno)]
pub async fn crud(c: TestContext) {
    c.chisel.write(
        "models/dated.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api"
        export class Dated extends ChiselEntity {
            date: Date;
        }"#,
    );
    c.chisel.write(
        "routes/dates.ts",
        r#"
        import { Dated } from "../models/dated.ts";
        export default Dated.crud();
        "#,
    );
    c.chisel.write(
        "routes/read.ts",
        r#"
        import { Dated } from "../models/dated.ts";
        export default async function chisel(req: Request) {
            let dated = (await Dated.findOne({}))!;
            return dated.date.toUTCString();
        }
        "#,
    );
    c.chisel.apply_ok().await;
    c.chisel
        .post_json(
            "/dev/dates",
            json!({
                "date": 1662624988000i64
            }),
        )
        .await;
    assert_eq!(
        c.chisel.get_text("/dev/read").await,
        "Thu, 08 Sep 2022 08:16:28 GMT"
    );
    json_is_subset(
        &c.chisel.get_json("/dev/dates?all=true").await,
        &json!({
            "results": [{
                "date": 1662624988000i64
            }]
        }),
    )
    .unwrap();

    json_is_subset(
        &c.chisel.get_json("/dev/dates?.date=1662624988000").await,
        &json!({
            "results": [{
                "date": 1662624988000i64
            }]
        }),
    )
    .unwrap();

    json_is_subset(
        &c.chisel.get_json("/dev/dates?.date=1662624988000.0").await,
        &json!({
            "results": [{
                "date": 1662624988000i64
            }]
        }),
    )
    .unwrap();
}
