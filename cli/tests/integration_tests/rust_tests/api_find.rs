use crate::framework::prelude::*;

async fn store_people(chisel: &Chisel) {
    chisel
        .post_json(
            "dev/store",
            json!({
                "first_name":"Glauber",
                "last_name":"Costa",
                "age": 666,
                "human": true,
                "height": 10.01
            }),
        )
        .await;
    chisel
        .post_json(
            "dev/store",
            json!({
                "first_name":"Jan",
                "last_name":"Plhak",
                "age": -666,
                "human": true,
                "height": 10.02
            }),
        )
        .await;
    chisel
        .post_json(
            "dev/store",
            json!({
                "first_name":"Pekka",
                "last_name":"Enberg",
                "age": 888,
                "human": false,
                "height": 12.2
            }),
        )
        .await;
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn find_many(c: TestContext) {
    c.chisel.copy_to_dir("examples/person.ts", "models");
    c.chisel.copy_to_dir("examples/store.ts", "routes");
    c.chisel.write(
        "routes/find_many.ts",
        r##"
        import { ChiselRequest } from "@chiselstrike/api"
        import { Person } from "../models/person.ts";

        export default async function chisel(req: ChiselRequest) {
            const use_predicate = req.query.getBool("use_predicate") ?? false;
            const use_expr = req.query.getBool("use_expr") ?? false;
            const first_name = req.query.get("first_name");
            if (first_name == undefined) {
                throw Error("first_name parameter must be specified.");
            }

            let filtered = undefined;
            if (use_expr) {
                filtered = await Person.__findMany(
                    p => p.first_name == first_name,
                    {
                        exprType: "Binary",
                        left: {
                        exprType: "Property",
                        property: "first_name",
                        object: {
                            exprType: "Parameter",
                            position: 0
                        }
                        },
                        op: "Eq",
                        right: {
                        exprType: "Value",
                        value: first_name
                        }
                    },
                    undefined,
                    1
                );
            } else if (use_predicate) {
                filtered = await Person.findMany(p => p.first_name == first_name, 1);
            } else {
                filtered = await Person.findMany({"first_name": first_name}, 1);
            }
            return filtered.map(p => p.first_name);
        }
    "##,
    );

    c.chisel.apply_ok().await;

    store_people(&c.chisel).await;

    for name in ["Glauber", "Jan", "Pekka"] {
        assert_eq!(
            c.chisel
                .get_json(&format!("/dev/find_many?first_name={name}"))
                .await,
            json!([name])
        );
        assert_eq!(
            c.chisel
                .get_json(&format!(
                    "/dev/find_many?first_name={name}&use_predicate=true"
                ))
                .await,
            json!([name])
        );
        assert_eq!(
            c.chisel
                .get_json(&format!("/dev/find_many?first_name={name}&use_expr=true"))
                .await,
            json!([name])
        );
    }
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn find_many_invalid_argument(c: TestContext) {
    c.chisel.copy_to_dir("examples/person.ts", "models");
    c.chisel.write(
        "routes/query.ts",
        r##"
        import { Person } from "../models/person.ts";

        export default async function chisel(req: Request) {
            let ret = "";
            const filtered = await Person.findMany({"foo": "bar"});
            filtered.forEach(row => {
                ret += row.first_name + " " + row.last_name + "\n";
            });
            return new Response(ret);
        }
    "##,
    );

    let mut output = c.chisel.apply_err().await;
    output.stderr
        .read("routes/query.ts:6:53 - error TS2769: No overload matches this call.")
        .read("Argument of type '{ foo: string; }' is not assignable to parameter of type 'Partial<Person>'");
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn find_one(c: TestContext) {
    c.chisel.copy_to_dir("examples/person.ts", "models");
    c.chisel.copy_to_dir("examples/store.ts", "routes");
    c.chisel.write(
        "routes/find_one.ts",
        r##"
        import { ChiselRequest } from "@chiselstrike/api"
        import { Person } from "../models/person.ts";

        export default async function chisel(req: ChiselRequest) {
            const use_predicate = req.query.getBool("use_predicate") ?? false;
            const use_expr = req.query.getBool("use_expr") ?? false;
            const first_name = req.query.get("first_name");
            if (first_name == undefined) {
                throw Error("first_name parameter must be specified.");
            }

            let the_one = undefined;
            if (use_expr) {
                the_one = await Person.__findOne(
                    p => p.first_name == first_name,
                    {
                        exprType: "Binary",
                        left: {
                        exprType: "Property",
                        property: "first_name",
                        object: {
                            exprType: "Parameter",
                            position: 0
                        }
                        },
                        op: "Eq",
                        right: {
                        exprType: "Value",
                        value: first_name
                        }
                    }
                );
            } else if (use_predicate) {
                the_one = await Person.findOne(p => p.first_name == first_name);
            } else {
                the_one = await Person.findOne({"first_name": first_name});
            }
            let name = "undefined";
            if (the_one !== undefined) {
                name = the_one.first_name
            }
            return name;
        }
    "##,
    );

    c.chisel.apply_ok().await;

    store_people(&c.chisel).await;

    for name in ["Glauber", "Jan", "Pekka"] {
        assert_eq!(
            c.chisel
                .get_text(&format!("/dev/find_one?first_name={name}"))
                .await,
            name,
        );
        assert_eq!(
            c.chisel
                .get_text(&format!(
                    "/dev/find_one?first_name={name}&use_predicate=true"
                ))
                .await,
            name,
        );
        assert_eq!(
            c.chisel
                .get_text(&format!("/dev/find_one?first_name={name}&use_expr=true"))
                .await,
            name,
        );
    }
}

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn find_by(c: TestContext) {
    c.chisel.copy_to_dir("examples/person.ts", "models");
    c.chisel.copy_to_dir("examples/find_by.ts", "routes");
    c.chisel.copy_to_dir("examples/store.ts", "routes");

    c.chisel.apply_ok().await;

    store_people(&c.chisel).await;

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"first_name",
                "value":"Jan"
            }),
        )
        .await;
    assert_eq!(resp_txt, "Jan Plhak -666 true 10.02 ");

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"last_name",
                "value":"Costa"
            }),
        )
        .await;
    assert_eq!(resp_txt, "Glauber Costa 666 true 10.01 ");

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"last_name",
                "value":"bagr"
            }),
        )
        .await;
    assert_eq!(resp_txt, "");

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"age",
                "value":-666
            }),
        )
        .await;
    assert_eq!(resp_txt, "Jan Plhak -666 true 10.02 ");

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"human",
                "value":true
            }),
        )
        .await;
    assert_eq!(
        resp_txt,
        "Glauber Costa 666 true 10.01 Jan Plhak -666 true 10.02 "
    );

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"height",
                "value":10.01
            }),
        )
        .await;
    assert_eq!(resp_txt, "Glauber Costa 666 true 10.01 ");

    let resp_txt = c
        .chisel
        .post_json_text(
            "/dev/find_by",
            json!({
                "field_name":"height",
            }),
        )
        .await;
    assert_eq!(
        resp_txt,
        "Glauber Costa 666 true 10.01 Jan Plhak -666 true 10.02 Pekka Enberg 888 false 12.2 "
    );

    let resp_text = c
        .chisel
        .post("/dev/find_by")
        .json(json!({
            "field_name":"misspelled_field_name",
            "value":10.01
        }))
        .send()
        .await
        .assert_status(500)
        .text();
    assert!(resp_text.contains(
        "Error: expression error: entity 'Person' doesn't have field 'misspelled_field_name'"
    ));
}
