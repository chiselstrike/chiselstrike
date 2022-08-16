use crate::framework::prelude::*;

#[chisel_macros::test(modules = Deno)]
pub async fn test(c: TestContext) {
    c.chisel.write(
        "models/types.ts",
        r##"
        export class Foo extends Chisel.ChiselEntity {
            order: number = 0;
            numbers: number[] = [];
            strings: string[] = [];
            booleans: boolean[][] = [];
        }
    "##,
    );
    c.chisel.write(
        "endpoints/store.ts",
        r##"
        import { Foo } from "../models/types.ts";
        export default async function chisel(req: Request) {
            await Foo.build({
                order: 0,
                numbers: [1, 0, -1.01],
                strings: ["Hello", "World"],
                booleans: [[false], [true, false]]
            }).save();
        }
    "##,
    );
    c.chisel.write(
        "endpoints/get.ts",
        r##"
        import { Foo } from "../models/types.ts";
        export default async function chisel(req: Request) {
            return await Foo.findAll();
        }
    "##,
    );
    c.chisel.write(
        "endpoints/crud_foos.ts",
        r##"
        import { Foo } from "../models/types.ts";
        export default Foo.crud();
    "##,
    );

    c.chisel.apply().await.expect("chisel apply failed");

    c.chisel
        .post_json_ok("/dev/store", json!({}))
        .await;

    {
        let data = c.chisel.get_json("/dev/get").await;
        let entity = &data[0];
        assert_eq!(entity["order"], json!(0));
        assert_eq!(entity["numbers"], json!([1, 0, -1.01]));
        assert_eq!(entity["strings"], json!(["Hello", "World"]));
        assert_eq!(entity["booleans"], json!([[false], [true, false]]));
    }
    {
        let data = c.chisel.get_json("/dev/crud_foos").await;
        let entity = &data["results"][0];
        assert_eq!(entity["order"], json!(0));
        assert_eq!(entity["numbers"], json!([1, 0, -1.01]));
        assert_eq!(entity["strings"], json!(["Hello", "World"]));
        assert_eq!(entity["booleans"], json!([[false], [true, false]]));
    }

    c.chisel
        .post_json_ok(
            "/dev/crud_foos",
            json!({
                "order": 1,
                "numbers": [0, 1, 2],
                "strings": ["Sauna", "rlz"],
                "booleans": [[true], [false]]
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

        let entity1 = &data["results"][1];
        assert_eq!(entity1["order"], json!(1));
        assert_eq!(entity1["numbers"], json!([0, 1, 2]));
        assert_eq!(entity1["strings"], json!(["Sauna", "rlz"]));
        assert_eq!(entity1["booleans"], json!([[true], [false]]));
    }

    {
        let status = c.chisel
            .post_json_status(
                "/dev/crud_foos",
                json!({
                    "order": 1,
                    "numbers": ["1"],
                    "strings": ["correct"],
                    "booleans": [[true]]
                }),
            )
            .await;
        assert_eq!(status, 500);

        let status = c.chisel
            .post_json_status(
                "/dev/crud_foos",
                json!({
                    "order": 1,
                    "numbers": [1],
                    "strings": [true],
                    "booleans": [[true]]
                }),
            )
            .await;
        assert_eq!(status, 500);

        let status = c.chisel
            .post_json_status(
                "/dev/crud_foos",
                json!({
                    "order": 1,
                    "numbers": [1],
                    "strings": ["correct"],
                    "booleans": [[1]]
                }),
            )
            .await;
        assert_eq!(status, 500);
    }
}
