use crate::framework::prelude::*;

fn write_models(chisel: &Chisel) {
    chisel.write(
        "models/models.ts",
        r#"
        import { ChiselEntity, Id } from "@chiselstrike/api";

        export class Author extends ChiselEntity {
            name: string;
        }
        export class Book extends ChiselEntity {
            title: string;
            author: Id<Author>;
        }"#,
    );
}

#[self::test(modules = Deno)]
pub async fn basic(c: TestContext) {
    write_models(&c.chisel);
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { Book, Author } from '../models/models.ts';
        export default async function () {
            const kafka = await Author.create({
                name: "Kafka"
            });
            const trial = await Book.create({
                title: "The Trial",
                author: kafka.id
            });
        }"#,
    );
    c.chisel.write(
        "routes/get_trial_author.ts",
        r#"
        import { Book, Author } from '../models/models.ts';
        export default async function () {
            const trial = await Book.findOne({title: "The Trial"});
            const author = await Author.byId(trial!.author);
            return author.name;
        }"#,
    );

    c.chisel.apply_ok().await;
    c.chisel.post("/dev/store").send().await.assert_ok().text();
    c.chisel
        .get("/dev/get_trial_author")
        .send()
        .await
        .assert_text("Kafka");
}

#[self::test(modules = Deno)]
pub async fn multiple_authors(c: TestContext) {
    write_models(&c.chisel);
    c.chisel.write(
        "routes/store.ts",
        r#"
        import { Book, Author } from '../models/models.ts';
        export default async function () {
            const kafka = await Author.create({
                name: "Kafka"
            });
            const kundera = await Author.create({
                name: "Kundera"
            });

            const trial = await Book.create({
                title: "The Trial",
                author: kafka.id
            });
            const meta = await Book.create({
                title: "Metamorphosis",
                author: kafka.id
            });
            const joke = await Book.create({
                title: "The Joke",
                author: kundera.id
            });
        }"#,
    );

    c.chisel.write(
        "routes/get_joke_author.ts",
        r#"
        import { Book, Author } from '../models/models.ts';
        export default async function () {
            const joke = await Book.findOne({title: "The Joke"});
            const author = await Author.byId(joke!.author);
            return author.name;
        }"#,
    );

    c.chisel.apply_ok().await;
    c.chisel.post("/dev/store").send().await.assert_ok().text();
    c.chisel
        .get("/dev/get_joke_author")
        .send()
        .await
        .assert_text("Kundera");
}

#[self::test(modules = Deno)]
pub async fn id_of_unknown_entity(c: TestContext) {
    c.chisel.write(
        "models/models.ts",
        r#"
        import { ChiselEntity, Id } from "@chiselstrike/api";

        export class Book extends ChiselEntity {
            title: string;
            author: Id<Author>;
        }"#,
    );
    c.chisel.apply().await.unwrap_err().stderr.read(
        "field `author` of entity `Book` is of type `Id<Author>`, but entity `Author` is undefined",
    );
}

#[self::test(modules = Deno)]
pub async fn crud(c: TestContext) {
    write_models(&c.chisel);
    c.chisel.write(
        "routes/books.ts",
        r#"
        import { Book } from "../models/models.ts";
        export default Book.crud();
        "#,
    );
    c.chisel.write(
        "routes/authors.ts",
        r#"
        import { Author } from "../models/models.ts";
        export default Author.crud();
        "#,
    );
    c.chisel.apply_ok().await;

    let r = c
        .chisel
        .post("/dev/authors")
        .json(json!({"name": "Kundera"}))
        .send()
        .await
        .assert_ok()
        .json();
    let kundera_id = r["id"].as_str().unwrap();
    c.chisel
        .post_json(
            "/dev/books",
            json!({
                "title": "The Joke",
                "author": kundera_id,
            }),
        )
        .await;

    let books_json = c
        .chisel
        .get(&format!("/dev/books?author={kundera_id}"))
        .send()
        .await
        .assert_ok()
        .json();
    json_is_subset(
        &books_json,
        &json!({
            "results": [{
                "title": "The Joke",
                "author": kundera_id,
            }]
        }),
    )
    .unwrap()
}
