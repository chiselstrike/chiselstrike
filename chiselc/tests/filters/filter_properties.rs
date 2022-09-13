// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use super::*;
use chiselc::rewrite::Target;
use serde_json::Value;

#[test]
fn filter() {
    let compiled: Value = compile!(
        r#"
        await Person.cursor().filter({ name: "Pekka", age: 39 }).toArray();
        await Person.cursor().filter({ }).toArray();
        const property = "name";
        await Person.cursor().filter({ [property]: "Pekka" });
        await Person.cursor().filter((p) => { return p.age > 4 }).toArray();
        await Person.cursor().filter((p) => { return p.age < 4 || (p.age > 10 && p.age != 12) });
        await Person.cursor().filter((p) => { return true; });
        "#,
        "Person";
        Target::FilterProperties
    )
    .parse()
    .unwrap();

    let expected = serde_json::json!(
    [
        { "entity_name": "Person", "properties": ["name", "age"] },
        { "entity_name":"Person", "properties": ["age"] },
        { "entity_name": "Person", "properties": ["age"] }
    ]);

    assert_eq!(compiled, expected);
}

#[test]
fn find_many() {
    let compiled: Value = compile!(
        r#"
        await Person.findMany({ name: "Pekka", age: 39 });
        await Person.findMany({ });
        await Person.findMany((p) => { return p.age > 4 });
        await Person.findMany((p) => { return p.age < 4 || (p.age > 10 && p.age != 12) });
        await Person.findMany((p) => { return true; });
        const property = 'name';
        await Person.findMany({ [property]: "Pekka" });
        "#,
        "Person";
        Target::FilterProperties
    )
    .parse()
    .unwrap();

    let expected: Value = serde_json::json!([
        { "entity_name": "Person", "properties": ["name","age"] },
        { "entity_name": "Person", "properties":["age"] },
        { "entity_name": "Person", "properties":["age"] }
    ]);

    assert_eq!(compiled, expected);
}

#[test]
fn find_one() {
    let compiled: Value = compile!(
        r#"
        await Person.findOne({ name: "Pekka", age: 39 });
        await Person.findOne({ });
        await Person.findOne((p) => { return p.age > 4 });
        await Person.findOne((p) => { return p.age < 4 || (p.age > 10 && p.age != 12) });
        await Person.findOne((p) => { return true; });
        const property = 'name';
        await Person.findOne({ [property]: "Pekka" });"#,
        "Person";
        Target::FilterProperties
    )
    .parse()
    .unwrap();

    let expected: Value = serde_json::json!([
        { "entity_name": "Person", "properties": ["name","age"] },
        { "entity_name": "Person", "properties":["age"] },
        { "entity_name": "Person", "properties": ["age"] }
    ]);

    assert_eq!(compiled, expected);
}
