use crate::framework::prelude::*;

#[chisel_macros::test(modules = Node)]
pub async fn test(c: TestContext) {
    c.chisel.write(
        "endpoints/error.ts",
        r##"
        import { foo } from "https://foo.bar";

        export default async function chisel(req: Request) {
            return foo;
        }
    "##,
    );

    let mut output = c.chisel.apply_err().await;
    output
        .stderr
        .read("could not import endpoint code into the runtime")
        .read("chiseld cannot load module https://foo.bar/ at runtime");
}
