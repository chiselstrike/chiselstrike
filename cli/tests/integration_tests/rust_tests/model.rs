use crate::framework::prelude::*;

#[self::test(modules = Deno)]
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

#[self::test(modules = Deno)]
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

#[self::test(modules = Deno)]
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
        .read(r##"Error: field `name` needs a type annotation or a default value"##);
}

#[self::test(modules = Deno)]
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

#[self::test(modules = Deno)]
pub async fn unique_constraint(mut c: TestContext) {
    c.chisel.write(
        "routes/posts.ts",
        r#"
        import { BlogPost } from "../models/blog_post.ts";
        export default BlogPost.crud();"#,
    );
    c.chisel.write(
        "models/blog_post.ts",
        r#"
        import { ChiselEntity, unique } from "@chiselstrike/api"
        export class BlogPost extends ChiselEntity {
            @unique relUrl: string;
            content: string;
        }"#,
    );
    c.chisel.apply_ok().await;
    c.chisel
        .describe_ok()
        .await
        .stdout
        .read("@unique relUrl: string;");

    c.chisel
        .post_json(
            "/dev/posts",
            json!({"relUrl": "post.html", "content": "Hello World"}),
        )
        .await;
    c.chisel
        .post("/dev/posts")
        .json(json!({"relUrl": "post.html", "content": "Other World"}))
        .send()
        .await
        .assert_status(500);

    // Ensure that only one entry has been stored
    let results = c.chisel.get_json("/dev/posts").await;
    assert!(results["results"].as_array().unwrap().len() == 1);

    // Ensure that changes are persisted.
    c.restart_chiseld().await;
    c.chisel
        .describe_ok()
        .await
        .stdout
        .read("@unique relUrl: string;");
}

#[self::test(modules = Deno)]
pub async fn field_type_from_initializer(c: TestContext) {
    c.chisel.write(
        "models/book.ts",
        r#"
        import { ChiselEntity } from "@chiselstrike/api";
        export class Book extends ChiselEntity {
            author = 'Austen';
        }"#,
    );
    c.chisel.write(
        "routes/test.ts",
        r#"
        import { Book } from '../models/book.ts';
        export default function () {
            const book = new Book();
            return book.author === 'Austen';
        }"#,
    );
    c.chisel.apply_ok().await;
    c.chisel
        .get("/dev/test")
        .send()
        .await
        .assert_json(json!(true));
}
