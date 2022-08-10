use crate::framework::TestConfig;

pub async fn test_http_import(config: TestConfig) {
    let mut ctx = config.setup().await;
    let (chisel, _chiseld) = ctx.get_chisels();

    chisel.write(
        "routes/error.ts",
        r##"
        import { foo } from "https://foo.bar";

        export default async function chisel(req: Request) {
            return foo;
        }
    "##,
    );

    let err = chisel.apply().await.expect_err("chisel apply should have failed");
    err.stderr()
        .read("Could not apply the provided code")
        .read("chiseld cannot load module https://foo.bar/ at runtime");
}
