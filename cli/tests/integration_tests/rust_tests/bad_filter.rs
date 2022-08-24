use crate::framework::prelude::*;

#[chisel_macros::test(modules = Deno, optimize = Both)]
pub async fn test(c: TestContext) {
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
