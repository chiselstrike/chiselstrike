use anyhow::{anyhow, Context, Result};
use std::borrow::Borrow;

use crate::framework::prelude::*;
use crate::framework::Chisel;

async fn fetch_person(chisel: &Chisel) -> serde_json::Value {
    let mut person = chisel.get_json("/dev/find_person").await;

    person.as_object_mut().unwrap().remove("id");
    person
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn policies(c: TestContext) {
    c.chisel.copy_to_dir("examples/person.ts", "models");
    c.chisel.copy_to_dir("examples/store.ts", "endpoints");

    c.chisel.write(
        "endpoints/find_person.ts",
        r##"
        import { Person } from "../models/person.ts";
        export default async function chisel(req: Request) {
            return (await Person.findAll())[0];
        }
    "##,
    );

    let mut r = c.chisel.apply_ok().await;
    r.stdout.read("Model defined: Person");

    let pekka = json!({
        "first_name":"Pekka",
        "last_name":"Heidelberg",
        "age": 2147483647,
        "human": false,
        "height": 12742333
    });

    c.chisel.post_json_ok("dev/store", &pekka).await;

    assert_eq!(fetch_person(&c.chisel).await, pekka);

    // anonymize first_name and last_name
    c.chisel.write(
        "models/person.ts",
        r##"
        import { ChiselEntity, labels } from "@chiselstrike/api";

        export class Person extends ChiselEntity {
            @labels("L1", "L2", "L3") first_name: string;
            @labels("pii", "L2") last_name: string;
            @labels("L1", "L3") human: boolean;
            age: number;
            height: number;
        }
    "##,
    );
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
        fetch_person(&c.chisel).await,
        json!({
            "first_name":"xxxxx",
            "last_name":"xxxxx",
            "age": 2147483647,
            "human": false,
            "height": 12742333
        })
    );

    // except_uri - exact match
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
        labels:
          - name: L2
            transform: anonymize
            except_uri: find_person
        "##,
    );
    c.chisel.apply_ok().await;
    assert_eq!(fetch_person(&c.chisel).await, pekka);

    // except_uri - regex match
    c.chisel.write_unindent(
        "policies/pol.yaml",
        r##"
        labels:
          - name: L2
            transform: anonymize
            except_uri: person$
        "##,
    );
    c.chisel.apply_ok().await;
    assert_eq!(fetch_person(&c.chisel).await, pekka);

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
        fetch_person(&c.chisel).await,
        json!({
            "first_name":"xxxxx",
            "last_name":"xxxxx",
            "age": 2147483647,
            "human": false,
            "height": 12742333
        })
    );

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
        fetch_person(&c.chisel).await,
        json!({
            "age": 2147483647,
            "human": false,
            "height": 12742333
        })
    );

    // test anonymization of related entities
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
        "endpoints/companies.ts",
        r##"
        import { crud } from "@chiselstrike/api";
        import { Company } from "../models/company.ts";

        export default Company.crud();
    "##,
    );
    c.chisel.apply_ok().await;

    c.chisel
        .post_json_ok(
            "dev/companies",
            json!({
                "name": "Chiselstrike",
                "ceo": {"firstName": "Glauber", "lastName": "Costa"},
                "accountant": {"firstName": "Edward", "lastName": "Ohare"},
                "secretSauce": "pumpkin"
            }),
        )
        .await;
    {
        let companies = c.chisel.get_json("/dev/companies").await;
        let company = &companies["results"].as_array().unwrap()[0];
        json_is_subset(
            company,
            json!({
                "name": "Chiselstrike",
                "ceo": {"firstName": "xxxxx", "lastName": "Costa"},
                "accountant": "xxxxx",
                "secretSauce": "xxxxx"
            }),
        )
        .unwrap();
    }
}

fn json_is_subset<V1, V2>(val: V1, subset: V2) -> Result<()>
where
    V1: Borrow<serde_json::Value>,
    V2: Borrow<serde_json::Value>,
{
    use serde_json::Value;
    let val = val.borrow();
    let subset = subset.borrow();

    match subset {
        Value::Object(sub_obj) => {
            let obj = val.as_object().context(anyhow!(
                "subset value is object but reference value is {val}"
            ))?;
            for (key, value) in sub_obj {
                let ref_value = obj
                    .get(key)
                    .context(anyhow!("reference object doesn't contain key `{key}`"))?;
                json_is_subset(ref_value, value)
                    .context(anyhow!("object properties `{key}` don't match"))?;
            }
        }
        Value::Array(sub_array) => {
            let arr = val.as_array().context(anyhow!(
                "subset value is array but reference value is {val}"
            ))?;
            anyhow::ensure!(
                arr.len() == sub_array.len(),
                "arrays have different lengths"
            );
            for (i, e) in arr.iter().enumerate() {
                let sub_e = &sub_array[i];
                json_is_subset(e, sub_e)
                    .context(anyhow!("failed to match elements of array on position {i}"))?
            }
        }
        Value::Number(_) => {
            anyhow::ensure!(
                val.is_number(),
                "subset value is number but reference value is {val}",
            );
            anyhow::ensure!(val == subset);
        }
        Value::String(_) => {
            anyhow::ensure!(
                val.is_string(),
                "subset value is string but reference value is {val}",
            );
            anyhow::ensure!(val == subset);
        }
        Value::Bool(_) => {
            anyhow::ensure!(
                val.is_string(),
                "subset value is bool but reference value is {val}",
            );
            anyhow::ensure!(val == subset);
        }
        Value::Null => {
            anyhow::ensure!(
                val.is_null(),
                "subset value is null but reference value is {val}",
            );
        }
    }
    Ok(())
}
