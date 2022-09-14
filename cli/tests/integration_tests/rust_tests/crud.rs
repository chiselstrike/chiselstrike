// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::collections::HashMap;

use crate::framework::prelude::*;

static PERSON_MODEL: &str = r#"
    import { ChiselEntity, labels } from "@chiselstrike/api";

    export class Person extends ChiselEntity {
        first_name: string = "";
        last_name: string = "";
        age: number = 0;
        human: boolean = false;
        height: number = 1;
    }
"#;

static PEOPLE_CRUD: &str = r#"
    import { Person } from "../models/person.ts";
    export default Person.crud();
"#;

static DEJAN: Lazy<serde_json::Value> = Lazy::new(|| {
    json!({
        "first_name":"Dejan",
        "last_name":"Mircevski",
        "age": 42,
        "human": true,
        "height": 7
    })
});

static GLAUBER: Lazy<serde_json::Value> = Lazy::new(|| {
    json!({
        "first_name":"Glauber",
        "last_name":"Costa",
        "age": 666,
        "human": true,
        "height": 10.01
    })
});

static HONZA: Lazy<serde_json::Value> = Lazy::new(|| {
    json!({
        "first_name":"Honza",
        "last_name":"Spacek",
        "age": 314,
        "human": false,
        "height": 11.11
    })
});

static JAN: Lazy<serde_json::Value> = Lazy::new(|| {
    json!({
        "first_name":"Jan",
        "last_name":"Plhak",
        "age": -666,
        "human": true,
        "height": 10.02
    })
});

static PEKKA: Lazy<serde_json::Value> = Lazy::new(|| {
    json!({
        "first_name":"Pekka",
        "last_name":"Heisenberg",
        "age": 2147483647,
        "human": false,
        "height": 12742333
    })
});

async fn store_person(chisel: &Chisel, person: &serde_json::Value) -> String {
    let resp = chisel
        .post("/dev/people")
        .json(person)
        .send()
        .await
        .assert_ok()
        .json();
    resp["id"].as_str().unwrap().into()
}

async fn store_all_people(chisel: &Chisel) -> HashMap<&'static str, String> {
    let mut ids = HashMap::default();
    ids.insert("Dejan", store_person(chisel, &DEJAN).await);
    ids.insert("Glauber", store_person(chisel, &GLAUBER).await);
    ids.insert("Pekka", store_person(chisel, &PEKKA).await);
    ids.insert("Honza", store_person(chisel, &HONZA).await);
    ids.insert("Jan", store_person(chisel, &JAN).await);
    ids
}

#[chisel_macros::test(modules = Deno)]
pub async fn basic(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;

    assert_eq!(
        c.chisel.get_json("/dev/people").await,
        json!({"results": []})
    );

    c.chisel.post_json("/dev/people", &*JAN).await;
    json_is_subset(
        &c.chisel.get_json("/dev/people").await,
        &json!({"results": [*JAN]}),
    )
    .unwrap();

    c.chisel.post_json("/dev/people", &*JAN).await;
    json_is_subset(
        &c.chisel.get_json("/dev/people").await,
        &json!({"results": [*JAN, *JAN]}),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn sort(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;
    store_all_people(&c.chisel).await;

    json_is_subset(
        &c.chisel.get_json("/dev/people?sort=first_name").await,
        &json!({"results": [*DEJAN, *GLAUBER, *HONZA, *JAN, *PEKKA]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel.get_json("/dev/people?sort=%2Blast_name").await,
        &json!({"results": [*GLAUBER, *PEKKA, *DEJAN, *JAN, *HONZA]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel.get_json("/dev/people?sort=-age").await,
        &json!({"results": [*PEKKA, *GLAUBER, *HONZA, *DEJAN, *JAN]}),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn limit_and_offset(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;
    store_all_people(&c.chisel).await;

    json_is_subset(
        &c.chisel
            .get_json("/dev/people?sort=first_name&limit=3")
            .await,
        &json!({"results": [*DEJAN, *GLAUBER, *HONZA]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?sort=-first_name&limit=3")
            .await,
        &json!({"results": [*PEKKA, *JAN, *HONZA]}),
    )
    .unwrap();

    json_is_subset(
        &c.chisel
            .get_json("/dev/people?sort=first_name&offset=3")
            .await,
        &json!({"results": [*JAN, *PEKKA]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?sort=-first_name&offset=3")
            .await,
        &json!({"results": [*GLAUBER, *DEJAN]}),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn limit_and_offset_ordering(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;
    store_all_people(&c.chisel).await;

    // Order of limit/offset parameters doesn't matter.
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?sort=first_name&limit=1&offset=2")
            .await,
        &json!({"results": [*HONZA]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?sort=first_name&offset=2&limit=1")
            .await,
        &json!({"results": [*HONZA]}),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn filters(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;
    store_all_people(&c.chisel).await;

    json_is_subset(
        &c.chisel.get_json("/dev/people?.last_name=Plhak").await,
        &json!({"results": [*JAN]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel.get_json("/dev/people?.age=2147483647").await,
        &json!({"results": [*PEKKA]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?.human=true&sort=first_name")
            .await,
        &json!({"results": [*DEJAN, *GLAUBER, *JAN]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?.human=false&sort=first_name")
            .await,
        &json!({"results": [*HONZA, *PEKKA]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?.human=true&.height=10.01")
            .await,
        &json!({"results": [*GLAUBER]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?sort=first_name&.age~lte=314&.height~gte=7.00001")
            .await,
        &json!({"results": [*HONZA, *JAN]}),
    )
    .unwrap();
    json_is_subset(
        &c.chisel
            .get_json("/dev/people?sort=first_name&.first_name~like=%25an")
            .await,
        &json!({"results": [*DEJAN, *JAN]}),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn get_by_id(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;
    let ids = store_all_people(&c.chisel).await;

    let jan_id = &ids["Jan"];
    let mut jan_with_id = JAN.clone();
    jan_with_id["id"] = json!(jan_id);
    c.chisel
        .get(&format!("/dev/people/{jan_id}"))
        .send()
        .await
        .assert_ok()
        .assert_json(&jan_with_id);

    let pekka_id = &ids["Pekka"];
    let mut pekka_with_id = PEKKA.clone();
    pekka_with_id["id"] = json!(pekka_id);
    c.chisel
        .get(&format!("/dev/people/{pekka_id}"))
        .send()
        .await
        .assert_ok()
        .assert_json(&pekka_with_id);

    c.chisel
        .get("/dev/people/whatever")
        .send()
        .await
        .assert_status(404);
}

#[chisel_macros::test(modules = Deno)]
pub async fn delete_by_id(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;
    let ids = store_all_people(&c.chisel).await;

    let jan_id = &ids["Jan"];
    c.chisel
        .delete(&format!("/dev/people/{jan_id}"))
        .send()
        .await
        .assert_ok();
    c.chisel
        .get(&format!("/dev/people/{jan_id}"))
        .send()
        .await
        .assert_status(404);

    json_is_subset(
        &c.chisel.get_json("/dev/people?sort=first_name").await,
        &json!({"results": [*DEJAN, *GLAUBER, *HONZA, *PEKKA]}),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn delete_with_filter(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;
    store_all_people(&c.chisel).await;

    c.chisel
        .delete("/dev/people?.first_name~like=%25an")
        .send()
        .await
        .assert_ok();
    json_is_subset(
        &c.chisel.get_json("/dev/people?sort=first_name").await,
        &json!({"results": [*GLAUBER, *HONZA, *PEKKA]}),
    )
    .unwrap();

    c.chisel
        .delete("/dev/people?all=true")
        .send()
        .await
        .assert_ok();
    json_is_subset(
        &c.chisel.get_json("/dev/people").await,
        &json!({"results": []}),
    )
    .unwrap();
}

#[chisel_macros::test(modules = Deno)]
pub async fn put_with_id(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;

    let person_id = "cef5d492-d7e3-4c45-9a55-5929b9ab8292";
    c.chisel
        .put(&format!("/dev/people/{person_id}"))
        .json(&*PEKKA)
        .send()
        .await
        .assert_ok();
    json_is_subset(
        &c.chisel.get_json("/dev/people").await,
        &json!({"results": [*PEKKA]}),
    )
    .unwrap();

    c.chisel
        .put(&format!("/dev/people/{person_id}"))
        .json(&*GLAUBER)
        .send()
        .await
        .assert_ok();
    json_is_subset(
        &c.chisel.get_json("/dev/people").await,
        &json!({"results": [*GLAUBER]}),
    )
    .unwrap();

    c.chisel
        .put("/dev/people/")
        .json(&*GLAUBER)
        .send()
        .await
        .assert_status(400);
}

#[chisel_macros::test(modules = Deno)]
pub async fn patch_with_id(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;

    let jan_id = store_person(&c.chisel, &JAN).await;

    c.chisel
        .patch(&format!("/dev/people/{jan_id}"))
        .json(json!({"height": 34.56}))
        .send()
        .await
        .assert_ok();

    let mut higher_jan = JAN.clone();
    higher_jan["height"] = json!(34.56);

    json_is_subset(
        &c.chisel.get_json("/dev/people").await,
        &json!({ "results": [higher_jan] }),
    )
    .unwrap();

    c.chisel
        .patch("/dev/people/")
        .json(json!({"height": 34.56}))
        .send()
        .await
        .assert_status(400);

    c.chisel
        .patch("/dev/people/foobar")
        .json(json!({"height": 34.56}))
        .send()
        .await
        .assert_status(404);
}

#[chisel_macros::test(modules = Deno)]
pub async fn paging(c: TestContext) {
    c.chisel.write("models/person.ts", PERSON_MODEL);
    c.chisel.write("routes/people.ts", PEOPLE_CRUD);
    c.chisel.apply_ok().await;
    store_all_people(&c.chisel).await;

    let r = c
        .chisel
        .get_json("/dev/people?sort=first_name&page_size=2")
        .await;
    json_is_subset(&r, &json!({"results": [*DEJAN, *GLAUBER]})).unwrap();

    let next_page = r["next_page"].as_str().unwrap();
    let r = c.chisel.get_json(next_page).await;
    json_is_subset(&r, &json!({"results": [*HONZA, *JAN]})).unwrap();

    let next_page = r["next_page"].as_str().unwrap();
    let r = c.chisel.get_json(next_page).await;
    json_is_subset(&r, &json!({"results": [*PEKKA]})).unwrap();

    let prev_page = r["prev_page"].as_str().unwrap();
    let r = c.chisel.get_json(prev_page).await;
    json_is_subset(&r, &json!({"results": [*HONZA, *JAN]})).unwrap();
}
