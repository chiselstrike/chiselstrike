// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::collections::HashMap;

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

async fn store_person(chisel: &Chisel, p: serde_json::Value) -> String {
    let r = chisel.post("dev/people").json(p).send().await.json();
    let id = r["id"].as_str().unwrap();
    id.to_owned()
}

async fn store_people(chisel: &Chisel) -> HashMap<String, String> {
    let glauber_id = store_person(
        chisel,
        json!({
            "firstName":"Glauber",
            "lastName":"Costa",
            "age": 666,
            "human": true,
            "height": 10.01
        }),
    )
    .await;
    let jan_id = store_person(
        chisel,
        json!({
            "firstName":"Jan",
            "lastName":"Plhak",
            "age": -666,
            "human": true,
            "height": 10.02
        }),
    )
    .await;
    let pekka_id = store_person(
        chisel,
        json!({
            "firstName":"Pekka",
            "lastName":"Enberg",
            "age": 888,
            "human": false,
            "height": 12.2
        }),
    )
    .await;
    HashMap::from([
        ("glauber".to_owned(), glauber_id),
        ("jan".to_owned(), jan_id),
        ("pekka".to_owned(), pekka_id),
    ])
}

fn with_client(c: &TestContext, src: &str) -> String {
    let common_funs = r#"
        async function iterToArray<T>(iterable: AsyncIterable<T>): Promise<T[]> {{
            const arr = [];
            for await (const e of iterable) {{
                arr.push(e)
            }}
            return arr;
        }}
    "#;
    match c.client_mode {
        ClientMode::Deno => {
            let imports = r#"
                import { createChiselClient } from "./client.ts";
                import { type GetParams } from "./client_lib.ts";
                import { assertEquals } from "https://deno.land/std@0.167.0/testing/asserts.ts";
            "#;
            format!(
                r#"
                {imports}
                {common_funs}

                const cli = createChiselClient('http://{}');
                {src}
            "#,
                c.chisel.api_address
            )
        }
        ClientMode::Node => {
            let imports = r#"
                import { createChiselClient } from "./client";
                import { type GetParams } from "./client_lib";

                function assertEquals<T>(actual: T, expected: T, msg?: string) {
                    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
                        throw new Error(msg ?? `actual (${actual}) != expected (${expected})`);
                    }
                }
            "#;
            format!(
                r#"
                {imports}
                {common_funs}
                async function main() {{
                    const cli = createChiselClient('http://{}');
                    {src}
                }}
                main();
            "#,
                c.chisel.api_address
            )
        }
    }
}

#[chisel_macros::test(modules = Deno, client_modes = Both)]
pub async fn get_simple(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);

    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    c.chisel.generate_ok("generated").await;
    let src = with_client(
        &c,
        r#"
        const ppl = (await cli.people.get({pageSize: 3})).results;
        const names = ppl.map(p => p.firstName);
        names.sort();
        assertEquals(names, ["Glauber", "Jan", "Pekka"]);
        "#,
    );
    c.ts_runner.run_ok("generated/test.ts", &src).await;
}

#[chisel_macros::test(modules = Deno, client_modes = Both)]
pub async fn get_all(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);

    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    c.chisel.generate_ok("generated").await;
    let src = with_client(
        &c,
        r#"
        const ppl = await cli.people.getAll({limit: 1});
        assertEquals(ppl.length, 1);
        "#,
    );
    c.ts_runner.run_ok("generated/test.ts", &src).await;
}

#[chisel_macros::test(modules = Deno, client_modes = Both)]
pub async fn get_by_id(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);

    c.chisel.apply_ok().await;
    let ids = store_people(&c.chisel).await;
    let jan_id = ids.get("jan").unwrap();

    c.chisel.generate_ok("generated").await;
    let src = with_client(
        &c,
        &format!(
            "
            const person = await cli.people.id('{jan_id}').get();
            assertEquals(person.id, '{jan_id}');
            assertEquals(person.firstName, 'Jan');
            ",
        ),
    );
    c.ts_runner.run_ok("generated/test.ts", &src).await;
}

#[chisel_macros::test(modules = Deno, client_modes = Both)]
pub async fn get_iterable_simple(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);

    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    c.chisel.generate_ok("generated").await;
    let src = with_client(
        &c,
        r#"
        const ppl = cli.people.getIter();
        const names = (await iterToArray(ppl)).map(p => p.firstName);
        names.sort();
        assertEquals(names, ["Glauber", "Jan", "Pekka"]);
        "#,
    );
    c.ts_runner.run_ok("generated/test.ts", &src).await;
}

#[chisel_macros::test(modules = Deno, client_modes = Both)]
pub async fn get_iterable_repeated(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);

    c.chisel.apply_ok().await;
    store_people(&c.chisel).await;

    c.chisel.generate_ok("generated").await;
    let src = with_client(
        &c,
        r#"
        async function runTest() {
            const ppl = cli.people.getIter({pageSize: 1});
            const names = (await iterToArray(ppl)).map(p => p.firstName);
            names.sort();
            assertEquals(names, ["Glauber", "Jan", "Pekka"]);
        }
        await runTest();
        await runTest();
        await runTest();
        "#,
    );
    c.ts_runner.run_ok("generated/test.ts", &src).await;
}
