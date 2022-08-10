use crate::framework::TestConfig;
use crate::framework::{IntegrationTest, OpMode};

#[chisel_macros::test(mode = OpMode::Deno)]
pub async fn test_bad_filter(config: TestConfig) {
    let mut ctx = config.setup().await;
    let (chisel, _chiseld) = ctx.get_chisels();

    chisel.copy_to_dir("examples/person.ts", "models");
    chisel.write(
        "endpoints/query.ts",
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

    let err = chisel.apply().expect_err("chisel apply should have failed");
    err.stderr()
        .read("endpoints/query.ts:6:53 - error TS2769: No overload matches this call.")
        .read("Argument of type '{ foo: string; }' is not assignable to parameter of type 'Partial<Person>'");
}
