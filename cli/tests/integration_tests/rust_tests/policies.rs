use crate::framework::prelude::*;
use crate::framework::{json_is_subset, Chisel};

async fn fetch_person(chisel: &Chisel) -> serde_json::Value {
    let mut person = chisel.get_json("/dev/find_person").await;

    person.as_object_mut().unwrap().remove("id");
    person
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn policies(c: TestContext) {
    c.chisel.copy_to_dir("examples/person.ts", "models");
    c.chisel.copy_to_dir("examples/store.ts", "routes");

    c.chisel.write(
        "routes/find_person.ts",
        r##"
        import { Person } from "../models/person.ts";
        export default async function chisel(req: Request) {
            return (await Person.findAll())[0];
        }
    "##,
    );
    c.chisel.apply_ok().await;

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
        "routes/companies.ts",
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
