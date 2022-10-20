
// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

static MODELS: &str = r#"
    import { ChiselEntity } from '@chiselstrike/api';

    export class Biography extends ChiselEntity {
        title: string = "";
        page_count: number = 0;
    }

    export class Person extends ChiselEntity {
        name: string = "bob";
        age: number = 0;
        biography: Biography = new Biography();
    }
"#;

static PEOPLE_CRUD: &str = r#"
    import { Person } from "../models/models.ts";
    export default Person.crud();
"#;

async fn store_people(chisel: &Chisel) {
    chisel
        .post_json(
            "dev/people",
            json!({
                "name": "Glauber",
                "age": 30,
                "biography":{
                    "title": "My life with elephants",
                    "page_count": 10
                }
            }),
        )
        .await;
    chisel
        .post_json(
            "dev/people",
            json!({
                "name": "Pekka",
                "age": 40,
                "biography":{
                    "title": "How sports didn't affect my life",
                    "page_count": 20
                }
            }),
        )
        .await;
    chisel
        .post_json(
            "dev/people",
            json!({
                "name": "Jan",
                "age": 50,
                "biography":{
                    "title": "The importance of being erinaceous",
                    "page_count": 30
                }
            }),
        )
        .await;
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn min_max_by(c: TestContext) {
    c.chisel.write("models/models.ts", MODELS);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.write(
        "routes/compute_aggregations.ts",
        r#"
        import { Person } from "../models/models.ts";

        export default async function chisel(req: Request) {
            const url = new URL(req.url);
            const min_age_str = url.searchParams.get("min_age") ?? undefined;
            const max_age_str = url.searchParams.get("max_age") ?? undefined;

            let ppl = Person.cursor();
            if (min_age_str !== undefined) {
                const min_age = Number(min_age_str);
                ppl = ppl.filter(p => p.age >= min_age);
            }
            if (max_age_str !== undefined) {
                const max_age = Number(max_age_str);
                ppl = ppl.filter(p => p.age <= max_age);
            }

            const aggregations = [
                await ppl.minBy("age"),
                await ppl.maxBy("age"),
            ];
            return aggregations;
        }"#,
    );
    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    assert_eq!(
        c.chisel.get_json("/dev/compute_aggregations").await,
        json!([30, 50])
    );
    assert_eq!(
        c.chisel.get_json("/dev/compute_aggregations?min_age=40").await,
        json!([40, 50])
    );
    assert_eq!(
        c.chisel.get_json("/dev/compute_aggregations?max_age=40").await,
        json!([30, 40])
    );
    assert_eq!(
        c.chisel.get_json("/dev/compute_aggregations?min_age=30&max_age=30").await,
        json!([30, 30])
    );
    assert_eq!(
        c.chisel.get_json("/dev/compute_aggregations?min_age=100").await,
        json!([null, null])
    );
}

#[chisel_macros::test(modules = Deno)]
pub async fn count_basic(c: TestContext) {
    c.chisel.write("models/models.ts", MODELS);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.write(
        "routes/count.ts",
        r#"
        import { Person } from "../models/models.ts";

        export default async function chisel(req: Request) {
            return await Person.cursor().count()
        }"#,
    );
    c.chisel.apply_ok().await;

    assert_eq!(
        c.chisel.get_json("/dev/count").await,
        json!(0)
    );

    store_people(&c.chisel).await;
    assert_eq!(
        c.chisel.get_json("/dev/count").await,
        json!(3)
    );

    c.chisel.write(
        "routes/count.ts",
        r#"
        import { Person } from "../models/models.ts";

        export default async function chisel(req: Request) {
            return await Person.cursor()
            .filter({age: 50})
            .count()
        }"#,
    );
    c.chisel.apply_ok().await;
    assert_eq!(
        c.chisel.get_json("/dev/count").await,
        json!(1)
    );
}

#[chisel_macros::test(modules = Deno, optimize = No)]
pub async fn count_in_typescript(c: TestContext) {
    c.chisel.write("models/models.ts", MODELS);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.write(
        "routes/count.ts",
        r#"
        import { Person } from "../models/models.ts";

        export default async function chisel(req: Request) {
            return await Person.cursor()
                .filter((p) => { return p.age >= 40 })
                .count();
        }"#,
    );
    c.chisel.apply_ok().await;

    store_people(&c.chisel).await;
    assert_eq!(
        c.chisel.get_json("/dev/count").await,
        json!(2)
    );
}
