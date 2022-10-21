"use strict";
t.context("migrate", async (t) => {
    const db = (await Db.create()).closeWith(t);
    await t.context("from empty", async (t) => {
        const oldLayout = {entityTables: [], schema: {entities: []}};

        await t.case("add simple entity", async (t) => {
            const newSchema = {
                entities: [
                    {
                        name: {user: "Book"},
                        idType: "string",
                        fields: [
                            {name: "title", type: {primitive: "string"}},
                            {name: "pageCount", type: {optional: {primitive: "number"}}, optional: true},
                        ],
                    },
                ],
            };

            const newLayout = await db.migrate(oldLayout, newSchema);
            const conn = (await Conn.connect(db, newLayout)).closeWith(t);
            const storeQ = Query.storeWithId(conn, {user: "Book"}).closeWith(conn);
            const findQ = Query.findById(conn, {user: "Book"}).closeWith(conn);

            const ctx = (await conn.begin()).closeWith(conn);
            await storeQ.execute(ctx, {id: "pride-prejudice", title: "Pride and Prejudice"});
            await storeQ.execute(ctx, {id: "john-galt", title: "Atlas Shrugged", pageCount: 1000});
            assertJsonEq(
                await findQ.fetchOne(ctx, "pride-prejudice"),
                {id: "pride-prejudice", title: "Pride and Prejudice", pageCount: undefined},
            );
            assertJsonEq(
                await findQ.fetchOne(ctx, "john-galt"),
                {id: "john-galt", title: "Atlas Shrugged", pageCount: 1000},
            );
        });
    });
});
