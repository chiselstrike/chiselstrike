// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;
use serde_json::json;

#[chisel_macros::test(modules = Deno, chiseld_args = ["--typescript-policies"])]
pub async fn typescript_policies(c: TestContext) {
    c.chisel.write(
        "models/person.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Person extends ChiselEntity {
            name: string;
            age: number;
        }
    "##,
    );
    c.chisel.write(
        "routes/person.ts",
        r##"
        import { Person } from "../models/person.ts";

        export default Person.crud();
    "##,
    );
    c.chisel.apply_ok().await;

    c.chisel
        .post_json("/dev/person", json!({ "name": "marin", "age": 27 }))
        .await;
    c.chisel
        .post_json("/dev/person", json!({ "name": "jim", "age": 40 }))
        .await;
    c.chisel
        .post_json("/dev/person", json!({ "name": "nathan", "age": 1 }))
        .await;

    c.chisel.write(
        "policies/Person.ts",
        r##"
        export default {
            read: (person, ctx) => {
                if (ctx.method == "POST" && person.name == "dean") {
                    return Action.Allow;
                }
                if (person.age > 30) {
                    return Action.Skip;
                } else if (person.name == "marin") {
                    return Action.Allow
                }
            }
        }
    "##,
    );

    c.chisel.apply_ok().await;

    let c = read_with_policies(c).await;
    let c = read_policy_deny_writes(c).await;

    c.chisel.write(
        "policies/Person.ts",
        r##"
        export default {
            read: (person, ctx) => {
                if (ctx.method == "POST" && person.name == "dean") {
                    return Action.Allow;
                }
                if (person.age > 30) {
                    return Action.Skip;
                } else if (person.name == "marin" || person.name == "changeMe") {
                    return Action.Allow
                }
            },
            onRead: (person, ctx) => {
                if (person.name == "changeMe") {
                    person.name = "changed";
                }

                return person;
            }
        }
    "##,
    );

    c.chisel.apply_ok().await;

    // check that read policies still work
    let c = read_with_policies(c).await;
    let c = check_read_transform(c).await;

    c.chisel.write(
        "policies/Person.ts",
        r##"
        export default {
            read: (person, ctx) => {
                if (person.age == 30) {
                    return Action.Deny;
                } else {
                    return Action.Allow;
                }
            },
            update: (person, ctx) => {
                if (person.name == "marin") {
                    return Action.Deny;
                } else {
                    return Action.Allow;
                }
            },
            create: (person, ctx) => {
                if (person.name == "marin") {
                    return Action.Allow;
                } else if (person.name == "peter") {
                    return Action.Deny;
                }
            }
        }
    "##,
    );
    c.chisel.apply_ok().await;

    let c = check_write_filter(c).await;

    c.chisel.write(
        "policies/Person.ts",
        r##"
        export default {
            onCreate: (person, ctx) => {
                if (person.name == "changeAge") {
                    person.age = 100;
                }
                return person;
            },
            onUpdate: (person, ctx) => {
                if (person.name = "changeAge") {
                    person.age = 200;
                }

                return person;
            }
        }
    "##,
    );

    c.chisel.apply_ok().await;

    check_write_transform(c).await;
}

async fn read_with_policies(c: TestContext) -> TestContext {
    // jim is skiped
    let res = c.chisel.get_json("/dev/person?.age~gt=10").await;
    assert_eq!(res["results"].as_array().unwrap().len(), 1);
    assert_eq!(res["results"][0]["name"], "marin");

    // refused
    let status = c
        .chisel
        .get("/dev/person?.name=nathan")
        .send()
        .await
        .status();
    assert_eq!(status, 403);

    c
}

async fn read_policy_deny_writes(c: TestContext) -> TestContext {
    // POST with age < 30 should be denied
    let status = c
        .chisel
        .post_json_status("/dev/person", json!({ "name": "john", "age": 20 }))
        .await;

    assert_eq!(status, 403);

    let status = c
        .chisel
        .post_json_status("/dev/person", json!({ "name": "dean", "age": 20 }))
        .await;

    c.chisel
        .delete("/dev/person?.name=dean")
        .send()
        .await
        .assert_ok();

    assert_eq!(status, 200);

    c
}

async fn check_read_transform(c: TestContext) -> TestContext {
    c.chisel
        .post_json("/dev/person", json!({ "name": "changeMe", "age": 20 }))
        .await;
    let res = c.chisel.get_json("/dev/person?.name=changeMe").await;

    assert_eq!(res["results"].as_array().unwrap().len(), 1);
    assert_eq!(res["results"][0]["name"], "changed");

    c
}

async fn check_write_filter(c: TestContext) -> TestContext {
    // try to update marin
    let marin_id = c.chisel.get_json("/dev/person?.name=marin").await["results"][0]["id"]
        .as_str()
        .map(ToString::to_string)
        .unwrap();

    let status = c
        .chisel
        .patch_json_status(&format!("/dev/person/{marin_id}"), json!({"age": 193}))
        .await;

    assert_eq!(status, 403);

    // create marin is ok
    c.chisel
        .post_json("/dev/person", json!({"name": "marin", "age": 193}))
        .await;

    // create james is not ok
    let status = c
        .chisel
        .post_json_status("/dev/person", json!({"name": "james", "age": 193}))
        .await;

    assert_eq!(status, 403);

    c
}

async fn check_write_transform(c: TestContext) -> TestContext {
    // create marin is ok
    let resp = c
        .chisel
        .post_json_response("/dev/person", json!({"name": "changeAge", "age": 193}))
        .await;

    resp.assert_ok();

    let json = resp.json();
    let id = json["id"].as_str().unwrap();

    let age = c.chisel.get_json("/dev/person?.name=changeAge").await["results"][0]["age"]
        .as_u64()
        .unwrap();

    assert_eq!(age, 100);

    c.chisel
        .patch_json(
            &format!("/dev/person/{id}"),
            json!({"name": "changeAge", "age": 193}),
        )
        .await;

    let age = c.chisel.get_json("/dev/person?.name=changeAge").await["results"][0]["age"]
        .as_u64()
        .unwrap();

    assert_eq!(age, 200);

    c
}

#[chisel_macros::test(modules = Deno, chiseld_args = ["--typescript-policies"])]
pub async fn read_ctx_headers(c: TestContext) {
    c.chisel.write(
        "models/person.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Person extends ChiselEntity {
            userId: number;
        }
    "##,
    );
    c.chisel.write(
        "routes/person.ts",
        r##"
        import { Person } from "../models/person.ts";

        export default Person.crud();
    "##,
    );
    c.chisel.apply_ok().await;

    c.chisel
        .post_json("/dev/person", json!({ "userId": 1 }))
        .await;

    c.chisel.write(
        "policies/Person.ts",
        r##"
        export default {
            read: (person, ctx) => {
                if (person.userId == ctx.headers["userid"]) {
                    return Action.Allow;
                } else {
                    return Action.Skip;
                }
            }
        }
    "##,
    );

    c.chisel.apply_ok().await;

    let results = c
        .chisel
        .get("/dev/person")
        .header("userId", "1")
        .send()
        .await;
    assert_eq!(results.json()["results"].as_array().unwrap().len(), 1);

    let results = c.chisel.get("/dev/person").send().await;
    assert!(results.json()["results"].as_array().unwrap().is_empty());
}
