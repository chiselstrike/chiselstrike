(async () => {

const schema = {
    entities: [
        {
            name: {user: "Book"},
            idType: "String",
            fields: [],
        }
    ],
};

const layout = {
    entityTables: [
        {
            entityName: {user: "Book"},
            tableName: "book",
            idCol: {colName: "id", repr: "StringAsText"},
            fieldCols: [],
        },
    ],
    schema,
};

await t.context("simple", async (t) => {
    await Conn.connect(layout, async (conn) => {
        await conn.executeSql(
            "CREATE TABLE book (id TEXT PRIMARY KEY)",
        );
        const findQuery = Query.findById(conn, {user: "Book"}).closeWith(conn);
        const storeQuery = Query.storeWithId(conn, {user: "Book"}).closeWith(conn);

        await t.case("store and fetch", async () => {
            await conn.begin(async (ctx) => {
                await storeQuery.startExecute({"id": "pride-prejudice"}, async (fut) => {
                    await fut.execute(ctx);
                    assertEq(fut.rowsAffected(), 1);
                });

                await findQuery.startFetch("pride-prejudice", async (stream) => {
                    assert(await stream.fetch(ctx));
                    assertJsonEq(stream.read(), {"id": "pride-prejudice"});
                    assert(!await stream.fetch(ctx));
                });
            });

            assertJsonEq(
                await conn.fetchSql("SELECT (id) FROM book ORDER BY id"),
                [["pride-prejudice"]],
            );
        });

        await t.case("store multiple", async () => {
            await conn.begin(async (ctx) => {
                await storeQuery.execute(ctx, {"id": "pride-prejudice"});
                await storeQuery.execute(ctx, {"id": "sense-sensibility"});
                await storeQuery.execute(ctx, {"id": "robinson-crusoe"});
            });

            assertJsonEq(
                await conn.fetchSql("SELECT (id) FROM book ORDER BY id"),
                [["pride-prejudice"], ["robinson-crusoe"], ["sense-sensibility"]],
            );
        });

        await t.case("fetch nonexistent", async () => {
            await conn.begin(async (ctx) => {
                await storeQuery.execute(ctx, {"id": "pride-prejudice"});
            });

            await conn.begin(async (ctx) => {
                assertJsonEq(
                    await findQuery.fetch(ctx, "sense-sensibility"),
                    [],
                );
            });
        });
    });
});

})()
