use crate::framework::prelude::*;

#[chisel_macros::test(modules = Deno, optimize = Yes)]
pub async fn check_optimization(c: TestContext) {
    c.chisel.write(
        "models/person.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api";

        export class Person extends ChiselEntity {
            name: string = "";
        }
    "#,
    );
    c.chisel.write(
        "routes/query.ts",
        r#"
        import { ChiselRequest } from '@chiselstrike/api';
        import { Person } from "../models/person.ts";

        export default async function chisel(req: ChiselRequest) {
            const c = Person.cursor()
                .filter(p => p.name == "Peter")
            // @ts-ignore: We need to access the inner field
            if (c.inner.type != "InternalFilter") {
                throw Error("Chisel compiler didn't optimize filtering query");
            }
            return await c
                .map(p => p.name)
                .toArray();
        }"#,
    );
    let output = c.chisel.apply_ok().await;
    assert!(!output.contains("no ChiselStrike compiler"));
    c.chisel.get("/dev/query").send().await.assert_ok();
}
