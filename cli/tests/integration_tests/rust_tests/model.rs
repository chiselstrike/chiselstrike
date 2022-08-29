use crate::framework::prelude::*;

#[chisel_macros::test(modules = Deno)]
pub async fn unknown_type(c: TestContext) {
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Bad extends ChiselEntity { a: SomeType; }
    "##,
    );
    c.chisel
        .apply_err()
        .await
        .stderr
        .read(r##"Error: field 'a' in class 'Bad' is of unknown entity type 'SomeType'"##);
}

#[chisel_macros::test(modules = Deno)]
pub async fn malformed_field_type(c: TestContext) {
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Malformed extends ChiselEntity { name: string(); }
    "##,
    );
    c.chisel
        .apply_err()
        .await
        .stderr
        .read(r##"error: Unexpected token `(`. Expected identifier, string literal, numeric literal or [ for the computed key"##)
        .read(r##"export class Malformed extends ChiselEntity { name: string(); }"##);
}

#[chisel_macros::test(modules = Deno)]
pub async fn missing_field_type(c: TestContext) {
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Malformed extends ChiselEntity { name; }
    "##,
    );
    c.chisel
        .apply_err()
        .await
        .stderr
        .read(r##"error: While parsing class Malformed"##)
        .read(r##"Error: type annotation is temporarily mandatory"##);
}

#[chisel_macros::test(modules = Deno)]
pub async fn duplicate_fields(c: TestContext) {
    c.chisel.write(
        "models/model.ts",
        r##"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Foo extends ChiselEntity { a?: string; a: string; }
    "##,
    );
    c.chisel.apply_err().await;
}
