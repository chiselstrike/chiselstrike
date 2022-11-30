// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;

static SOME_MODEL: &str = r#"
    import { ChiselEntity } from "@chiselstrike/api";

    export class SomeModel extends ChiselEntity {
        a: string = "";
    }
"#;

#[chisel_macros::test(modules = Deno)]
pub async fn assign_to_nonexistent_field(c: TestContext) {
    c.chisel.write("models/model.ts", SOME_MODEL);
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { SomeModel } from "../models/model.ts";

        export default async function chisel(req: Request) {
            const mod = new SomeModel();
            mod.invalidField = "foo";
            await mod.save();
            return "ok";
        }"#,
    );
    c.chisel
        .apply_err()
        .await
        .stderr
        .read("Error: Could not compile routes")
        .read("Caused by:")
        .read("Compilation failed:")
        .read("error TS2339: Property 'invalidField' does not exist on type 'SomeModel'.");
}

#[chisel_macros::test(modules = Deno)]
pub async fn cant_save_in_get(c: TestContext) {
    c.chisel.write("models/model.ts", SOME_MODEL);
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { SomeModel } from "../models/model.ts";

        export default async function chisel(req: Request) {
            const mod = new SomeModel();
            mod.a = "foo";
            await mod.save();
            return "ok";
        }"#,
    );
    c.chisel.apply_ok().await;
    c.chisel
        .get("/dev/store/")
        .send()
        .await
        .assert_status(500)
        .assert_text_contains("Error: Mutating the backend is not allowed during GET");
    c.chisel.post("/dev/store/").send().await.assert_ok();
}

#[chisel_macros::test(modules = Deno)]
pub async fn repeated_save_calls(c: TestContext) {
    c.chisel.write("models/model.ts", SOME_MODEL);
    c.chisel.write(
        "routes/get_data.ts",
        r#"
        import { SomeModel } from "../models/model.ts";

        export default async function chisel(req: Request) {
            const data = await SomeModel.cursor().toArray();
            return data.map(d => d.a);
        }

        "#,
    );
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { SomeModel } from "../models/model.ts";

        export default async function chisel(req: Request) {
            const mod = new SomeModel();
            mod.a = "foo";
            await mod.save();
            await mod.save();
            await mod.save();
            return "ok";
        }"#,
    );
    c.chisel.apply_ok().await;
    c.chisel.post("/dev/store/").send().await.assert_ok();
    c.chisel
        .get("/dev/get_data")
        .send()
        .await
        .assert_json(json!(["foo"]));
}
