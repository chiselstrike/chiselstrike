"use strict";
(async () => {

const schema = {
    entities: [
        {
            name: {user: "Book"},
            idType: "string",
            fields: [
                {name: "title", type: {primitive: "string"}},
                {name: "author", type: {optional: {primitive: "string"}}, optional: true},
            ],
        }
    ],
};

const layout = {
    entityTables: [
        {
            entityName: {user: "Book"},
            tableName: "book",
            idCol: {colName: "id", repr: "stringAsText"},
            fieldCols: [
                {fieldName: "title", colName: "title", repr: "stringAsText", nullable: false},
                {fieldName: "author", colName: "author", repr: "stringAsText", nullable: true},
            ],
        },
    ],
    schema,
};

await t.context("simple", async (t) => {
    const db = (await Db.create()).closeWith(t);
    await db.executeSql(
        "CREATE TABLE book (id TEXT PRIMARY KEY, title TEXT NOT NULL, author TEXT)",
    );

    const conn = (await Conn.connect(db, layout)).closeWith(db);
    const findQuery = Query.findById(conn, {user: "Book"}).closeWith(conn);
    const storeQuery = Query.storeWithId(conn, {user: "Book"}).closeWith(conn);

    await t.case("store and fetch", async () => {
        await conn.begin(async (ctx) => {
            const bookObj = {
                "id": "pride-prejudice",
                "title": "Pride and Prejudice",
                "author": "Jane Austen",
            };

            await storeQuery.newExecute(bookObj, async (fut) => {
                await fut.execute(ctx);
                assertEq(fut.rowsAffected(), 1);
            });

            await findQuery.newFetch("pride-prejudice", async (stream) => {
                assert(await stream.fetch(ctx));
                assertJsonEq(stream.read(), bookObj);
                assert(!await stream.fetch(ctx));
            });
        });

        assertJsonEq(
            await db.fetchSql("SELECT id FROM book ORDER BY id"),
            [["pride-prejudice"]],
        );
    });

    await t.case("store multiple", async () => {
        await conn.begin(async (ctx) => {
            await storeQuery.execute(ctx, {"id": "pap", "title": "Pride and Prejudice", "author": "Austen"});
            await storeQuery.execute(ctx, {"id": "sas", "title": "Sense and Sensibility"});
            await storeQuery.execute(ctx, {"id": "robinson", "title": "Robinson Crusoe"});
        });

        assertJsonEq(
            await db.fetchSql("SELECT id, title, author FROM book ORDER BY id"),
            [
                ["pap", "Pride and Prejudice", "Austen"],
                ["robinson", "Robinson Crusoe", null],
                ["sas", "Sense and Sensibility", null],
            ],
        );
    });

    await t.case("fetch nonexistent", async () => {
        await conn.begin(async (ctx) => {
            await storeQuery.execute(ctx, {"id": "pap", "title": "Pride and Prejudice"});
        });

        await conn.begin(async (ctx) => {
            assertJsonEq(await findQuery.fetch(ctx, "sas"), []);
        });
    });
});

})()
