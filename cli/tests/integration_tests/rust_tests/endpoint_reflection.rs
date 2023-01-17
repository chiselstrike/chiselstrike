// SPDX-FileCopyrightText: © 2023 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[chisel_macros::test(modules = Node)]
pub async fn basic(c: TestContext) {
    // Check that reflection works with no type arguments for request variable.
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap, ChiselRequest } from '@chiselstrike/api';

        export default new RouteMap()
            .post('/foo', async (req) => {
                return "Hello World";
            });
        "#,
    );
    c.chisel.apply_ok().await;
    c.chisel
        .post("/dev/foo")
        .send()
        .await
        .assert_text("Hello World");

    // Check that reflection works with no type arguments.
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap, ChiselRequest } from '@chiselstrike/api';

        export default new RouteMap()
            .post('/foo', async (req: ChiselRequest) => {
                return "Hello World";
            });
        "#,
    );
    c.chisel.apply_ok().await;
    c.chisel
        .post("/dev/foo")
        .send()
        .await
        .assert_text("Hello World");
}

#[chisel_macros::test(modules = Node)]
pub async fn query_validation(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap, ChiselRequest } from '@chiselstrike/api';

        type Query = {
            count: number;
            name: string;
            human: boolean;
        };

        export default new RouteMap()
            .post('/foo', async (req: ChiselRequest<Query>) => {
                const q = req.typedQuery();
                const count: number = q.count;
                const name: string = q.name;
                const human: boolean = q.human;
                return [count, name, human];
            });
        "#,
    );

    c.chisel.apply_ok().await;
    c.chisel
        .post("/dev/foo?count=42&name=Jan&human=true")
        .send()
        .await
        .assert_json(json!([42, "Jan", true]));

    // Wrong type
    c.chisel
        .post("/dev/foo?count=foo&name=Jan&human=true")
        .send()
        .await
        .assert_status(400)
        .assert_text_contains("'count' must be of type 'number' but isn't");

    // Missing argument
    c.chisel
        .post("/dev/foo?name=Jan&human=true")
        .send()
        .await
        .assert_status(400)
        .assert_text_contains("required request query parameter missing: count");

    // Extra parameter
    c.chisel
        .post("/dev/foo?count=42&name=Jan&human=true&extra=foo")
        .send()
        .await
        .assert_json(json!([42, "Jan", true]));
}

#[chisel_macros::test(modules = Node)]
pub async fn json_body_validation(c: TestContext) {
    c.chisel.write(
        "routes/index.ts",
        r#"
        import { RouteMap, ChiselRequest } from '@chiselstrike/api';

        type Company = {
            name: string
        };

        type Body = {
            name: string,
            age: number,
            human: boolean,
            birthDate: Date,
            employer: Company,
            formerEmployers: Company[],
            school: {
                founder: string
            },
        };

        export default new RouteMap()
            .post('/foo', async (req: ChiselRequest<{}, Body>) => {
                const b = await req.typedJson();
                const name: string = b.name;
                const age: number = b.age;
                const human: boolean = b.human;
                const birthDate: Date = b.birthDate;
                const employer: Company = b.employer;
                const formerEmployers: Company[] = b.formerEmployers;
                const school: {founder: string} = b.school;
                return {name, age, human, birthDate, employer, formerEmployers, school};
            });
        "#,
    );

    let data = json!({
        "name": "Jan",
        "age": 42,
        "human": true,
        "birthDate": 999,
        "employer": {"name": "YourNameHere"},
        "formerEmployers": [{"name": "Lulz"}],
        "school": {
            "founder": "Ruprt"
        },
    });

    c.chisel.apply_ok().await;
    c.chisel
        .post("/dev/foo")
        .json(&data)
        .send()
        .await
        .assert_json(&data);

    c.chisel
        .post("/dev/foo")
        .json(json!({
            "name": 10,
            "age": 42,
            "human": true,
            "birthDate": 999,
            "employer": {"name": "YourNameHere"},
            "formerEmployers": [{"name": "Lulz"}],
            "school": {
                "founder": "Ruprt"
            },
        }))
        .send()
        .await
        .assert_status(400)
        .assert_text_contains("expected 'string' at .name, but provided value is of type 'number'");

    c.chisel
        .post("/dev/foo")
        .json(json!({
            "name": "Jan",
            "age": "foo",
            "human": true,
            "birthDate": 999,
            "employer": {"name": "YourNameHere"},
            "formerEmployers": [{"name": "Lulz"}],
            "school": {
                "founder": "Ruprt"
            },
        }))
        .send()
        .await
        .assert_status(400)
        .assert_text_contains("expected 'number' at .age, but provided value is of type 'string'");

    c.chisel
        .post("/dev/foo")
        .json(json!({
            "name": "Jan",
            "age": 42,
            "human": 1,
            "birthDate": 999,
            "employer": {"name": "YourNameHere"},
            "formerEmployers": [{"name": "Lulz"}],
            "school": {
                "founder": "Ruprt"
            },
        }))
        .send()
        .await
        .assert_status(400)
        .assert_text_contains(
            "expected 'boolean' at .human, but provided value is of type 'number'",
        );

    c.chisel
        .post("/dev/foo")
        .json(json!({
            "name": "Jan",
            "age": 42,
            "human": true,
            "birthDate": "asd",
            "employer": {"name": "YourNameHere"},
            "formerEmployers": [{"name": "Lulz"}],
            "school": {
                "founder": "Ruprt"
            },
        }))
        .send()
        .await
        .assert_status(400)
        .assert_text_contains("failed to convert value to Date for '.birthDate'");

    c.chisel
        .post("/dev/foo")
        .json(json!({
            "name": "Jan",
            "age": 42,
            "human": true,
            "birthDate": 999,
            "employer": "{'name': 'YourNameHere'}",
            "formerEmployers": [{"name": "Lulz"}],
            "school": {
                "founder": "Ruprt"
            },
        }))
        .send()
        .await
        .assert_status(400)
        .assert_text_contains(
            "expected Object (anonymousObject) at '.employer', but provided value is of type 'string'",
        );

    c.chisel
        .post("/dev/foo")
        .json(json!({
            "name": "Jan",
            "age": 42,
            "human": true,
            "birthDate": 999,
            "employer": {"name": "YourNameHere"},
            "formerEmployers": {"name": "Lulz"},
            "school": {
                "founder": "Ruprt"
            },
        }))
        .send()
        .await
        .assert_status(400)
        .assert_text_contains(
            "expected Array at '.formerEmployers', but provided value is of type 'object'",
        );

    c.chisel
        .post("/dev/foo")
        .json(json!({
            "name": "Jan",
            "age": 42,
            "human": true,
            "birthDate": 999,
            "employer": {"name": "YourNameHere"},
            "formerEmployers": [{"name": "Lulz"}],
            "school": [{
                "founder": "Ruprt"
            }],
        }))
        .send()
        .await
        .assert_status(400)
        .assert_text_contains(
            "expected Object (anonymousObject) at '.school', but provided value is of type 'Array'",
        );
}
