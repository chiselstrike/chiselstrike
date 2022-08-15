use crate::framework::prelude::*;

pub async fn test(c: TestContext) {
    c.chisel.write(
        "routes/error.ts",
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
        .read("Could not apply the provided code")
        .read("chiseld cannot load module https://foo.bar/ at runtime");
}
