use crate::framework::{IntegrationTest, OpMode, TestConfig};

#[chisel_macros::test(mode = OpMode::Node)]
pub async fn test_http_import(config: TestConfig) {
    let mut ctx = config.setup().await;
    let (chisel, _chiseld) = ctx.get_chisels();

    chisel.write(
        "endpoints/error.ts",
        r##"
        import { foo } from "https://foo.bar";

        export default async function chisel(req: Request) {
            return foo;
        }
    "##,
    );

    let err = chisel
        .apply()
        .await
        .expect_err("chisel apply should have failed");
    err.stderr()
        .read("could not import endpoint code into the runtime")
        .read("chiseld cannot load module https://foo.bar/ at runtime");
}
