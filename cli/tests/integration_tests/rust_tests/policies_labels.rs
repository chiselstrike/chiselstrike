// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

async fn store_person(chisel: &Chisel, person: &serde_json::Value) -> String {
    let resp = chisel
        .post("/dev/persons")
        .json(person)
        .send()
        .await
        .assert_ok()
        .json();
    resp["id"].as_str().unwrap().into()
}

async fn fetch_person(chisel: &Chisel, id: &str) -> serde_json::Value {
    let mut person = chisel.get_json(&format!("/dev/persons/{}", id)).await;
    person.as_object_mut().unwrap().remove("id");
    person
}

static PERSONS_ROUTE: &str = r##"
    import { Person } from "../models/person.ts";
    export default Person.crud();
    "##;

static PERSON: &str = r##"
    import { ChiselEntity, labels } from "@chiselstrike/api";

    export class Person extends ChiselEntity {
        first_name: string = "";
        @labels("pii") last_name: string = "";
        age: number = 0;
        human: boolean = false;
        height: number = 1;
    }
    "##;

static PERSON_WITH_LABELS: &str = r##"
    import { ChiselEntity, labels } from "@chiselstrike/api";

    export class Person extends ChiselEntity {
        @labels("L1", "L2", "L3") first_name: string;
        @labels("pii", "L2") last_name: string;
        @labels("L1", "L3") human: boolean;
        age: number;
        height: number;
    }
    "##;

lazy_static::lazy_static! {
    static ref PEKKA: serde_json::Value = json!({
        "first_name":"Pekka",
        "last_name":"Heidelberg",
        "age": 2147483647,
        "human": false,
        "height": 12742333
    });
}

lazy_static::lazy_static! {
    static ref GLAUBER: serde_json::Value = json!({
        "first_name":"Glauber",
        "last_name":"Costa",
        "age": 666,
        "human": true,
        "height": 10.01
    });
}

#[self::test(modules = Deno, optimize = Yes)]
async fn no_policy(c: TestContext) {
    c.chisel.write_unindent("routes/persons.ts", PERSONS_ROUTE);
    c.chisel.write_unindent("models/person.ts", PERSON);
    c.chisel.apply_ok().await;

    let pekka_id = store_person(&c.chisel, &PEKKA).await;
    assert_eq!(fetch_person(&c.chisel, &pekka_id).await, *PEKKA);
}

#[self::test(modules = Deno, optimize = Both)]
async fn transform_anonymize(c: TestContext) {
    c.chisel.write_unindent("routes/persons.ts", PERSONS_ROUTE);
    c.chisel
        .write_unindent("models/person.ts", PERSON_WITH_LABELS);
    c.chisel.apply_ok().await;
    let pekka_id = store_person(&c.chisel, &PEKKA).await;

    // anonymize first_name and last_name
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
        labels:
          - name: Linf
            transform: anonymize
          - name: L1
          - name: L2
            transform: anonymize
        "##,
    );
    c.chisel.apply_ok().await;
    assert_eq!(
        fetch_person(&c.chisel, &pekka_id).await,
        json!({
            "first_name":"xxxxx",
            "last_name":"xxxxx",
            "age": 2147483647,
            "human": false,
            "height": 12742333
        })
    );

    // except_uri - simple regex match
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
        labels:
          - name: L2
            transform: anonymize
            except_uri: persons
        "##,
    );
    c.chisel.apply_ok().await;
    assert_eq!(fetch_person(&c.chisel, &pekka_id).await, *PEKKA);

    // except_uri - advanced regex match
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
        labels:
          - name: L2
            transform: anonymize
            except_uri: 'sons/[0-9a-f-]+$'
        "##,
    );
    c.chisel.apply_ok().await;
    assert_eq!(fetch_person(&c.chisel, &pekka_id).await, *PEKKA);

    // except_uri - no regex match
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
        labels:
          - name: L2
            transform: anonymize
            except_uri: ^no_match
        "##,
    );
    c.chisel.apply_ok().await;
    assert_eq!(
        fetch_person(&c.chisel, &pekka_id).await,
        json!({
            "first_name":"xxxxx",
            "last_name":"xxxxx",
            "age": 2147483647,
            "human": false,
            "height": 12742333
        })
    );
}

#[self::test(modules = Deno, optimize = Both)]
async fn transform_omit(c: TestContext) {
    c.chisel.write_unindent("routes/persons.ts", PERSONS_ROUTE);
    c.chisel
        .write_unindent("models/person.ts", PERSON_WITH_LABELS);
    c.chisel.apply_ok().await;
    let pekka_id = store_person(&c.chisel, &PEKKA).await;

    // test omit transformation
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
        labels:
          - name: L2
            transform: omit
        "##,
    );
    c.chisel.apply_ok().await;
    assert_eq!(
        fetch_person(&c.chisel, &pekka_id).await,
        json!({
            "age": 2147483647,
            "human": false,
            "height": 12742333
        })
    );
}

#[self::test(modules = Deno, optimize = Both)]
async fn transform_anonymize_related_entities(c: TestContext) {
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
        labels:
          - name: pii
            transform: anonymize
        "##,
    );
    c.chisel.write(
        "models/company.ts",
        r##"
        import { ChiselEntity, labels } from "@chiselstrike/api";

        export class Human extends ChiselEntity {
            @labels("pii") firstName: string = "";
            lastName: string = "";
        }
        export class Company extends ChiselEntity {
            name: string = "";
            ceo: Human;
            @labels("pii") accountant: Human = new Human();
            @labels("pii") secretSauce: string = "AAAA";
        }
    "##,
    );
    c.chisel.write(
        "routes/companies.ts",
        r##"
        import { crud } from "@chiselstrike/api";
        import { Company } from "../models/company.ts";

        export default Company.crud();
    "##,
    );
    c.chisel.apply_ok().await;

    c.chisel
        .post_json(
            "dev/companies",
            json!({
                "name": "Chiselstrike",
                "ceo": {"firstName": "Glauber", "lastName": "Costa"},
                "accountant": {"firstName": "Edward", "lastName": "Ohare"},
                "secretSauce": "pumpkin"
            }),
        )
        .await;

    let companies = c.chisel.get_json("/dev/companies").await;
    let company = &companies["results"].as_array().unwrap()[0];
    json_is_subset(
        company,
        &json!({
            "name": "Chiselstrike",
            "ceo": {"firstName": "xxxxx", "lastName": "Costa"},
            "accountant": "xxxxx",
            "secretSauce": "xxxxx"
        }),
    )
    .unwrap();
}

#[self::test(modules = Deno, optimize = Both)]
async fn persistence_after_restart(mut c: TestContext) {
    c.chisel.write(
        "models/models.ts",
        r##"
        export class TestLabelsPersist1 extends ChiselEntity {
            @labels("a", "b") one: string;
            @labels("a") two: string;
        }
        export class TestLabelsPersist2 extends ChiselEntity {
            @labels("c", "b") three: string;
            @labels("a") four: string;
        }
    "##,
    );
    c.chisel.apply_ok().await;
    c.restart_chiseld().await;
    let mut stdout = c.chisel.describe_ok().await.stdout;
    stdout
        .read(r##"@labels("a", "b") one: string;"##)
        .read(r##"@labels("a") two: string;"##)
        .read(r##"@labels("c", "b") three: string;"##)
        .read(r##"@labels("a") four: string;"##);
}

#[self::test(modules = Deno, optimize = Both)]
async fn anonymize_and_filter(c: TestContext) {
    c.chisel.write("routes/persons.ts", PERSONS_ROUTE);
    c.chisel.write("models/person.ts", PERSON);
    // anonymize last_name
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r#"
        labels:
          - name: pii
            transform: anonymize
        "#,
    );

    c.chisel.apply_ok().await;

    store_person(&c.chisel, &PEKKA).await;
    store_person(&c.chisel, &GLAUBER).await;

    json_is_subset(
        &c.chisel.get_json("/dev/persons?sort=first_name").await,
        &json!({"results": [
            {"first_name": "Glauber", "last_name": "xxxxx"},
            {"first_name": "Pekka", "last_name": "xxxxx"}
        ]}),
    )
    .unwrap();

    let r = c
        .chisel
        .get_json("/dev/persons?sort=first_name&.first_name=Pekka")
        .await;
    assert!(r["results"].as_array().unwrap().len() == 1);
    json_is_subset(
        &r,
        &json!({"results": [
            {"first_name": "Pekka", "last_name": "xxxxx"}
        ]}),
    )
    .unwrap();

    // TODO: Is this correct? We are leaking the presence of Costa in the data.
    json_is_subset(
        &c.chisel
            .get_json("/dev/persons?sort=first_name&.last_name=Costa")
            .await,
        &json!({"results": [
            {"first_name": "Glauber", "last_name": "xxxxx"},
        ]}),
    )
    .unwrap();
}
