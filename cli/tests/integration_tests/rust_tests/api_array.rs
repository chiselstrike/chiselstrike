// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

#[chisel_macros::test(modules = Deno)]
pub async fn test(c: TestContext) {
    c.chisel.write(
        "models/types.ts",
        r##"
        import { ChiselEntity } from '@chiselstrike/api';
        export class Foo extends ChiselEntity {
            order: number = 0;
            numbers: number[] = [];
            strings: string[] = [];
            booleans: boolean[][] = [];
            dates: Date[] = [];
        }
    "##,
    );
    c.chisel.write(
        "routes/store.ts",
        r##"
        import { Foo } from "../models/types.ts";
        export default async function chisel(req: Request) {
            await Foo.build({
                order: 0,
                numbers: [1, 0, -1.01],
                strings: ["Hello", "World"],
                booleans: [[false], [true, false]],
                dates: [new Date(1662624988000)]
            }).save();
        }
    "##,
    );
    c.chisel.write(
        "routes/get.ts",
        r##"
        import { Foo } from "../models/types.ts";
        export default async function chisel(req: Request) {
            return await Foo.findAll();
        }
    "##,
    );
    c.chisel.write(
        "routes/crud_foos.ts",
        r##"
        import { Foo } from "../models/types.ts";
        export default Foo.crud();
    "##,
    );

    c.chisel.apply().await.expect("chisel apply failed");

    c.chisel.post_json("/dev/store", json!({})).await;

    {
        let data = c.chisel.get_json("/dev/get").await;
        let entity = &data[0];
        assert_eq!(entity["order"], json!(0));
        assert_eq!(entity["numbers"], json!([1, 0, -1.01]));
        assert_eq!(entity["strings"], json!(["Hello", "World"]));
        assert_eq!(entity["booleans"], json!([[false], [true, false]]));
        assert_eq!(entity["dates"], json!([1662624988000i64]));
    }
    {
        let data = c.chisel.get_json("/dev/crud_foos").await;
        let entity = &data["results"][0];
        assert_eq!(entity["order"], json!(0));
        assert_eq!(entity["numbers"], json!([1, 0, -1.01]));
        assert_eq!(entity["strings"], json!(["Hello", "World"]));
        assert_eq!(entity["booleans"], json!([[false], [true, false]]));
        assert_eq!(entity["dates"], json!([1662624988000i64]));
    }

    c.chisel
        .post_json(
            "/dev/crud_foos",
            json!({
                "order": 1,
                "numbers": [0, 1, 2],
                "strings": ["Sauna", "rlz"],
                "booleans": [[true], [false]],
                "dates": [42]
            }),
        )
        .await;
    {
        let data = c.chisel.get_json("/dev/crud_foos?sort=order").await;
        let entity0 = &data["results"][0];
        assert_eq!(entity0["order"], json!(0));
        assert_eq!(entity0["numbers"], json!([1, 0, -1.01]));
        assert_eq!(entity0["strings"], json!(["Hello", "World"]));
        assert_eq!(entity0["booleans"], json!([[false], [true, false]]));
        assert_eq!(entity0["dates"], json!([1662624988000i64]));

        let entity1 = &data["results"][1];
        assert_eq!(entity1["order"], json!(1));
        assert_eq!(entity1["numbers"], json!([0, 1, 2]));
        assert_eq!(entity1["strings"], json!(["Sauna", "rlz"]));
        assert_eq!(entity1["booleans"], json!([[true], [false]]));
        assert_eq!(entity1["dates"], json!([42]));
    }

    {
        c.chisel
            .post("/dev/crud_foos")
            .json(json!({
                "order": 1,
                "numbers": ["1"],
                "strings": ["correct"],
                "booleans": [[true]],
                "dates": [42]
            }))
            .send()
            .await
            .assert_status(500);

        c.chisel
            .post("/dev/crud_foos")
            .json(json!({
                "order": 1,
                "numbers": [1],
                "strings": [true],
                "booleans": [[true]],
                "dates": [42]
            }))
            .send()
            .await
            .assert_status(500);

        c.chisel
            .post("/dev/crud_foos")
            .json(json!({
                "order": 1,
                "numbers": [1],
                "strings": ["correct"],
                "booleans": [[1]],
                "dates": [42]
            }))
            .send()
            .await
            .assert_status(500);

        c.chisel
            .post("/dev/crud_foos")
            .json(json!({
                "order": 1,
                "numbers": [1],
                "strings": ["correct"],
                "booleans": [[true]],
                "dates": ["foo"]
            }))
            .send()
            .await
            .assert_status(500);
    }
}

#[chisel_macros::test(modules = Deno)]
pub async fn array_of_ids(c: TestContext) {
    c.chisel.write(
        "models/types.ts",
        r##"
        import { ChiselEntity, Id } from '@chiselstrike/api';

        export class Person extends ChiselEntity {
            name: string;
        }
        export class Company extends ChiselEntity {
            name: string;
            employees: Id<Person>[];
        }
    "##,
    );
    c.chisel.write(
        "routes/store.ts",
        r##"
        import { Person, Company } from "../models/types.ts";
        export default async function chisel(req: Request) {
            const jan = await Person.create({
                name: "Jan",
            });
            const person = await Company.create({
                name: "foo",
                employees: [jan.id],
            });
        }
    "##,
    );

    c.chisel.write(
        "routes/get.ts",
        r##"
        import { Company, Person } from "../models/types.ts";
        export default async function chisel(req: Request) {
            const company = await Company.findOne({name: "foo"});
            const janId = company!.employees![0];
            return await Person.findOne({id: janId});
        }
    "##,
    );

    c.chisel.apply().await.expect("chisel apply failed");

    c.chisel.post_json("/dev/store", json!({})).await;
    let jan = c.chisel.get_json("/dev/get").await;
    assert_eq!(jan["name"], "Jan");
}
