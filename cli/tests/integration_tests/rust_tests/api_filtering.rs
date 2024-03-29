// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;
use crate::framework::Chisel;

static PERSON_MODEL: &str = r#"
    import { ChiselEntity } from "@chiselstrike/api";

    export class Person extends ChiselEntity {
        firstName: string = "";
        lastName: string = "";
        age: number = 0;
        human: boolean = false;
        height: number = 1;
    }
"#;

static PEOPLE_CRUD: &str = r#"
    import { Person } from "../models/person.ts";
    export default Person.crud();
"#;

async fn store_people(chisel: &Chisel) {
    chisel
        .post_json(
            "dev/people",
            json!({
                "firstName":"Glauber",
                "lastName":"Costa",
                "age": 666,
                "human": true,
                "height": 10.01
            }),
        )
        .await;
    chisel
        .post_json(
            "dev/people",
            json!({
                "firstName":"Jan",
                "lastName":"Plhak",
                "age": -666,
                "human": true,
                "height": 10.02
            }),
        )
        .await;
    chisel
        .post_json(
            "dev/people",
            json!({
                "firstName":"Pekka",
                "lastName":"Enberg",
                "age": 888,
                "human": false,
                "height": 12.2
            }),
        )
        .await;
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn basic(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.write(
        "routes/query.ts",
        r#"
        import { ChiselRequest } from '@chiselstrike/api';
        import { Person } from "../models/person.ts";

        export default async function chisel(req: ChiselRequest) {
            const firstName = req.query.get("first_name")!;
            return await Person.cursor()
                .filter({firstName})
                .map(p => p.lastName)
                .toArray();
        }"#,
    );
    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    assert_eq!(
        c.chisel.get_json("/dev/query?first_name=Glauber").await,
        json!(["Costa"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/query?first_name=Pekka").await,
        json!(["Enberg"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/query?first_name=Jan").await,
        json!(["Plhak"])
    );
    assert_eq!(
        c.chisel.get_json("/dev/query?first_name=Dejan").await,
        json!([])
    );
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn various_types(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.write(
        "routes/query.ts",
        r#"
        import { ChiselRequest } from '@chiselstrike/api';
        import { Person } from "../models/person.ts";

        export default async function chisel(req: ChiselRequest) {
            let filter = {};

            const age = req.query.getNumber("age");
            const height = req.query.getNumber("height");
            const human = req.query.getBool("human");

            if (age !== undefined) {
                filter = {age};
            } else if (height !== undefined) {
                filter = {height};
            } else if (human !== undefined) {
                filter = {human};
            }

            return await Person.cursor()
                .filter(filter)
                .map(p => p.firstName)
                .toArray();
        }"#,
    );
    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    assert_eq!(
        c.chisel.get_json("/dev/query?age=888").await,
        json!(["Pekka"])
    );
    assert_eq!(c.chisel.get_json("/dev/query?age=8888").await, json!([]));
    assert_eq!(c.chisel.get_json("/dev/query?age=88").await, json!([]));

    assert_eq!(
        c.chisel.get_json("/dev/query?height=10.02").await,
        json!(["Jan"])
    );
    assert_eq!(c.chisel.get_json("/dev/query?height=10.2").await, json!([]));
    assert_eq!(c.chisel.get_json("/dev/query?height=10").await, json!([]));
    assert_eq!(c.chisel.get_json("/dev/query?height=1002").await, json!([]));
    assert_eq!(
        c.chisel.get_json("/dev/query?height=10.002").await,
        json!([])
    );

    assert_eq!(
        c.chisel.get_json("/dev/query?human=false").await,
        json!(["Pekka"])
    );
    let mut humans: Vec<String> =
        serde_json::from_value(c.chisel.get_json("/dev/query?human=true").await).unwrap();
    humans.sort();
    assert_eq!(humans, vec!["Glauber", "Jan"]);

    let mut names: Vec<String> =
        serde_json::from_value(c.chisel.get_json("/dev/query").await).unwrap();
    names.sort();
    assert_eq!(names, vec!["Glauber", "Jan", "Pekka"]);
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn take_and_filter_permutation(c: TestContext) {
    c.chisel.write(
        "models/foo.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Foo extends ChiselEntity {
            f1: string = "";
            f2: number = 0;
        }"#,
    );
    c.chisel.write(
        "routes/query.ts",
        r#"
        import { Foo } from '../models/foo.ts';

        export default async (req: Request) => {
            await Foo.build({ f1: "x", f2: 1 }).save();
            await Foo.build({ f1: "y", f2: 2 }).save();
            await Foo.build({ f1: "z", f2: 3 }).save();
            await Foo.build({ f1: "z", f2: 4 }).save();
            let c = await Foo.cursor().sortBy("f2");

            const simpleTake = await c
                .take(1)
                .toArray();
            const takeFilter = await c
                .take(3)
                .filter({ f1: "z" })
                .toArray();
            const filterTake = await c
                .filter({ f1: "z" })
                .take(1)
                .toArray();
            return {simpleTake, takeFilter, filterTake};
        }"#,
    );
    c.chisel.apply_ok().await;

    json_is_subset(
        &c.chisel.post("/dev/query").send().await.assert_ok().json(),
        &json!({
            "simpleTake": [{"f1": "x", "f2": 1}],
            "takeFilter": [{"f1": "z", "f2": 3}],
            "filterTake": [{"f1": "z", "f2": 3}],
        }),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn sql_keywords(c: TestContext) {
    c.chisel.write(
        "models/sql_keywords.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api";
        export class SqlKeywords extends ChiselEntity {
            limit: string = "limit";
            group: string = "group";
            where: string = "where";
            select: string = "select";
            delete: string = "delete";
            insert?: string;
            alter?: string;
        }"#,
    );
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { SqlKeywords } from '../models/sql_keywords.ts';
        export default async function chisel(req: Request) {
            await SqlKeywords.build({insert: "insert"}).save();
        }
        "#,
    );
    c.chisel.write(
        "routes/findall.ts",
        r#"
        import { ChiselRequest } from '@chiselstrike/api';
        import { SqlKeywords } from '../models/sql_keywords.ts';

        export default async function chisel(req: ChiselRequest) {
            const property = req.query.get("property")!;
            return await SqlKeywords.findMany(
                {[property]: property} as Partial<SqlKeywords>
            );
        }"#,
    );
    c.chisel.apply_ok().await;
    c.chisel.post("/dev/store").send().await.assert_ok();

    let keywords = vec!["limit", "group", "where", "select", "delete", "insert"];
    let expected_json = json!([{
        "limit": "limit",
        "group": "group",
        "where": "where",
        "select": "select",
        "delete": "delete",
        "insert": "insert",
    }]);

    for keyword in &keywords {
        let url = format!("/dev/findall?property={keyword}");
        json_is_subset(&c.chisel.get_json(&url).await, &expected_json).unwrap();
    }
}

static EXPR_FILTER_ENDPOINT: &str = r#"
    import { ChiselRequest } from '@chiselstrike/api';
    import { Person } from "../models/person.ts";

    export default async function chisel(req: ChiselRequest) {
        let c = Person.cursor();
        if (req.query.getBool("tsFiltering")) {
            // Enforces in-TS eval.
            c = c.filter((p) => p.id != btoa(p.id!))
        }
        return await c
            .filter(await req.json())
            .sortBy("firstName")
            .map(p => p.firstName)
            .toArray();
    }"#;

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn expr_filter_basic(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.write("routes/query.ts", EXPR_FILTER_ENDPOINT);
    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    let url = format!("/dev/query?tsFiltering={}", !c.optimized);
    c.chisel
        .post(&url)
        .json(json!({
            "lastName": "Plhak"
        }))
        .send()
        .await
        .assert_json(json!(["Jan"]));

    c.chisel
        .post(&url)
        .json(json!({
            "age": 666
        }))
        .send()
        .await
        .assert_json(json!(["Glauber"]));

    c.chisel
        .post(&url)
        .json(json!({
            "age": {"$gte": 666, "$lt": 667}
        }))
        .send()
        .await
        .assert_json(json!(["Glauber"]));
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn expr_filter_logical_ops(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.write("routes/query.ts", EXPR_FILTER_ENDPOINT);
    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    let url = format!("/dev/query?tsFiltering={}", !c.optimized);
    c.chisel
        .post(&url)
        .json(json!({
            "$not": {"firstName": "Glauber"}
        }))
        .send()
        .await
        .assert_json(json!(["Jan", "Pekka"]));

    c.chisel
        .post(&url)
        .json(json!({
            "$and": [
                {"age": {"$gte": 666}},
                {"human": true}
            ]
        }))
        .send()
        .await
        .assert_json(json!(["Glauber"]));

    c.chisel
        .post(&url)
        .json(json!({
            "$or": [
                {"age": 666},
                {"firstName": "Pekka"}
            ]
        }))
        .send()
        .await
        .assert_json(json!(["Glauber", "Pekka"]));

    c.chisel
        .post(&url)
        .json(json!({
            "$and": [
                {
                    "$or": [
                        {"firstName": "Jan"},
                        {"firstName": "Pekka"}
                    ]
                },
                {
                    "$or": [
                        {"lastName": "Costa"},
                        {"lastName": "Plhak"}
                    ]
                }
            ]
        }))
        .send()
        .await
        .assert_json(json!(["Jan"]));
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn expr_filter_comparators(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.write("routes/query.ts", EXPR_FILTER_ENDPOINT);
    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    let url = format!("/dev/query?tsFiltering={}", !c.optimized);
    c.chisel
        .post(&url)
        .json(json!({
            "firstName": {"$eq": "Jan"}
        }))
        .send()
        .await
        .assert_json(json!(["Jan"]));

    c.chisel
        .post(&url)
        .json(json!({
            "firstName": {"$ne": "Jan"}
        }))
        .send()
        .await
        .assert_json(json!(["Glauber", "Pekka"]));

    c.chisel
        .post(&url)
        .json(json!({
            "age": {"$gt": 666}
        }))
        .send()
        .await
        .assert_json(json!(["Pekka"]));

    c.chisel
        .post(&url)
        .json(json!({
            "age": {"$gte": 666}
        }))
        .send()
        .await
        .assert_json(json!(["Glauber", "Pekka"]));

    c.chisel
        .post(&url)
        .json(json!({
            "age": {"$lt": 666}
        }))
        .send()
        .await
        .assert_json(json!(["Jan"]));
    c.chisel
        .post(&url)
        .json(json!({
            "age": {"$lte": 666}
        }))
        .send()
        .await
        .assert_json(json!(["Glauber", "Jan"]));
}

#[chisel_macros::test(modules = Deno, optimize = Yes)]
pub async fn expr_filter_nested_entities(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write(
        "models/company.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api";
        import { Person } from "../models/person.ts";

        export class Company extends ChiselEntity {
            name: string;
            ceo: Person = new Person();
        }
    "#,
    );
    c.chisel.write(
        "routes/companies.ts",
        r#"
        import { Company } from "../models/company.ts";
        export default Company.crud();
    "#,
    );
    c.chisel.write(
        "routes/query.ts",
        r#"
        import { ChiselRequest } from '@chiselstrike/api';
        import { Company } from "../models/company.ts";

        export default async function chisel(req: ChiselRequest) {
            let c = Company.cursor();
            if (req.query.getBool("tsFiltering")) {
                // Enforces in-TS eval.
                c = c.filter((p) => p.id != btoa(p.id!))
            }
            return await c
                .filter(await req.json())
                .sortBy("name")
                .map(c => c.name)
                .toArray();
        }"#,
    );
    c.chisel.apply_ok().await;
    c.chisel
        .post_json(
            "dev/companies",
            json!({
                "name": "ChiselStrike",
                "ceo": {
                    "firstName":"Glauber",
                    "lastName":"Costa",
                    "age": 666,
                    "human": true,
                    "height": 10.01
                }
            }),
        )
        .await;
    c.chisel
        .post_json(
            "dev/companies",
            json!({
                "name": "Sauna inc.",
                "ceo": {
                    "firstName":"Pekka",
                    "lastName":"Enberg",
                    "age": 888,
                    "human": false,
                    "height": 12.2
                }
            }),
        )
        .await;

    let url = format!("/dev/query?tsFiltering={}", !c.optimized);
    c.chisel
        .post(&url)
        .json(json!({
            "ceo": {
                "firstName": "Glauber"
            }
        }))
        .send()
        .await
        .assert_json(json!(["ChiselStrike"]));
    c.chisel
        .post(&url)
        .json(json!({
            "ceo": {
                "age": {"$gt": 666}
            }
        }))
        .send()
        .await
        .assert_json(json!(["Sauna inc."]));
}
