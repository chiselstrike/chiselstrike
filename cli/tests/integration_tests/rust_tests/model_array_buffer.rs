// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

extern crate base64;
use crate::framework::prelude::*;

static STORAGE_MODEL: &str = r#"
    import { ChiselEntity } from "@chiselstrike/api";

    export class StorageEntity extends ChiselEntity {
        data: ArrayBuffer;
    }
"#;

static READ_ROUTE: &str = r#"
    import { StorageEntity } from "../models/storage.ts";

    export default async function chisel(req: Request) {
        const x = await StorageEntity.findOne({});
        const view = new Uint8Array(x!.data);
        const decoder = new TextDecoder('utf-8');
        return decoder.decode(view);
    }
"#;

fn write_files(chisel: &Chisel) {
    chisel.write("models/storage.ts", STORAGE_MODEL);
    chisel.write("routes/read.ts", READ_ROUTE);
    chisel.write(
        "routes/data.ts",
        r#"
        import { StorageEntity } from "../models/storage.ts";
        export default StorageEntity.crud();
    "#,
    );
}

#[chisel_macros::test(modules = Deno)]
pub async fn basic(c: TestContext) {
    write_files(&c.chisel);
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { StorageEntity } from "../models/storage.ts";

        export default async function chisel(req: Request) {
            const buff = new ArrayBuffer(5);
            const view = new Uint8Array(buff);
            view.set([107, 197, 175, 197, 136], 0);
            await StorageEntity.build({
                data: buff
            }).save();
        }"#,
    );
    c.chisel.apply_ok().await;

    c.chisel.post("/dev/store").send().await.assert_ok();
    c.chisel
        .get("/dev/read")
        .send()
        .await
        .assert_ok()
        .assert_text("kůň");

    let r = c.chisel.get_json("/dev/data").await;
    let base64_data = r["results"].as_array().unwrap()[0]["data"]
        .as_str()
        .unwrap();
    let text_bytes = base64::decode(base64_data).unwrap();
    let response_text = String::from_utf8_lossy(&text_bytes);
    assert_eq!(response_text, "kůň");
}

#[chisel_macros::test(modules = Deno)]
pub async fn save_buffer_view_to_buffer(c: TestContext) {
    write_files(&c.chisel);
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { StorageEntity } from "../models/storage.ts";

        export default async function chisel(req: Request) {
            const buff = new ArrayBuffer(5);
            const view = new Uint8Array(buff);
            view.set([107, 197, 175, 197, 136], 0);
            await StorageEntity.build({
                data: view
            }).save();
        }"#,
    );
    c.chisel.apply_ok().await;

    c.chisel.post("/dev/store").send().await.assert_ok();
    c.chisel
        .get("/dev/read")
        .send()
        .await
        .assert_ok()
        .assert_text("kůň");
}

#[chisel_macros::test(modules = Deno)]
pub async fn crud(c: TestContext) {
    write_files(&c.chisel);
    c.chisel.apply_ok().await;

    let encoded_horse = base64::encode("kůň".as_bytes());
    c.chisel
        .post_json("/dev/data", json!({ "data": encoded_horse }))
        .await;

    c.chisel
        .get("/dev/read")
        .send()
        .await
        .assert_ok()
        .assert_text("kůň");

    let r = c.chisel.get_json("/dev/data").await;
    let base64_data = r["results"].as_array().unwrap()[0]["data"]
        .as_str()
        .unwrap();
    let text_bytes = base64::decode(base64_data).unwrap();
    let response_text = String::from_utf8_lossy(&text_bytes);
    assert_eq!(response_text, "kůň");
}
// TODO: Use when arrays of buffers are implemented.
// #[chisel_macros::test(modules = Deno)]
// pub async fn array_of_buffers(c: TestContext) {
//     c.chisel.write(
//         "models/storage.ts",
//         r#"
//         import { ChiselEntity } from "@chiselstrike/api";
//         export class StorageEntity extends ChiselEntity {
//             data: ArrayBuffer[];
//         }
//         "#,
//     );
//     c.chisel.write(
//         "routes/store.ts",
//         r#"
//         import { StorageEntity } from "../models/storage.ts";
//         function makeBuffer(bytes: Array<number>) {
//             const buff = new ArrayBuffer(bytes.length);
//             const view = new Uint8Array(buff);
//             view.set(bytes, 0);
//             return buff;
//         }

//         export default async function chisel(req: Request) {
//             const array = [];
//             array.push(makeBuffer([107, 195, 189, 196, 141]));
//             array.push(makeBuffer([106, 97, 107]));
//             array.push(makeBuffer([98, 105, 196, 141]));

//             await StorageEntity.build({
//                 data: array
//             }).save();
//         }"#,
//     );
//     c.chisel.write(
//         "routes/read.ts",
//         r#"
//         import { StorageEntity } from "../models/storage.ts";

//         export default async function chisel(req: Request) {
//             const x = await StorageEntity.findOne({});
//             return x!.data.map(bytes => {
//                 const view = new Uint8Array(bytes);
//                 const decoder = new TextDecoder('utf-8');
//                 return decoder.decode(view);
//             });
//         }"#,
//     );
//     c.chisel.write(
//         "routes/data.ts",
//         r#"
//         import { StorageEntity } from "../models/storage.ts";
//         export default StorageEntity.crud();
//     "#,
//     );
//     c.chisel.apply_ok().await;

//     c.chisel.post("/dev/store").send().await.assert_ok();
//     c.chisel
//         .get("/dev/read")
//         .send()
//         .await
//         .assert_ok()
//         .assert_json(json!(["kýč", "jak", "bič"]));
// }
