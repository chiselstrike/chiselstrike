"use strict";
t.context("entity load and store", async (t) => {
    const db = (await Db.create()).closeWith(t);

    async function prepare(db, entity, table) {
        const layout = {
            entityTables: [table],
            schema: {entities: [entity]},
        };
        return (await Conn.connect(db, layout)).closeWith(db);
    }

    const uuid1 = "2d35d9c6-a42c-4ba9-b877-bed8ed61d02a";
    const uuid2 = "f6223176-fbd8-4551-9f1b-fc6b11504ee9";

    await t.case("uuid id", async (t) => {
        const conn = await prepare(db,
            {
                name: {user: "Book"},
                idType: "uuid",
                fields: [],
            },
            {
                entityName: {user: "Book"},
                tableName: "book",
                idCol: {colName: "id", repr: "uuidAsText"},
                fieldCols: [],
            },
        );

        await db.executeSql(
            "CREATE TABLE book (id TEXT PRIMARY KEY)",
            `INSERT INTO book (id) VALUES ('${uuid1}')`,
        );

        await assertFind(conn, {user: "Book"}, {"id": uuid1});
        await assertStoreAndFind(conn, {user: "Book"}, {"id": uuid2});
        await assertThrows("UUID", async () => {
            await storeWithId(conn, {user: "Book"}, {"id": "not-a-valid-uuid"});
        });
        await assertThrows("UUID", async () => {
            await storeWithId(conn, {user: "Book"}, {"id": "FE963C92-EE6C-4961-BE1A-77FB19CB09F9"});
        });
    });

    await t.case("primitives", async (t) => {
        const conn = await prepare(db,
            {
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "s", type: {primitive: "string"}},
                    {name: "n", type: {primitive: "number"}},
                    {name: "b", type: {primitive: "boolean"}},
                    {name: "u", type: {primitive: "uuid"}},
                    {name: "d", type: {primitive: "jsDate"}},
                ],
            },
            {
                entityName: {user: "E"},
                tableName: "e",
                idCol: {colName: "id", repr: "stringAsText"},
                fieldCols: [
                    {fieldName: "s", colName: "s", repr: "stringAsText"},
                    {fieldName: "n", colName: "n", repr: "numberAsDouble"},
                    {fieldName: "b", colName: "b", repr: "booleanAsInt"},
                    {fieldName: "u", colName: "u", repr: "uuidAsText"},
                    {fieldName: "d", colName: "d", repr: "jsDateAsDouble"},
                ],
            },
        );

        await db.executeSql(
            `CREATE TABLE e (
                id TEXT PRIMARY KEY,
                s TEXT NOT NULL,
                n REAL NOT NULL,
                b INTEGER NOT NULL,
                u TEXT NOT NULL,
                d REAL NOT NULL
            )`,
            `INSERT INTO e VALUES ('one', 'first str', 42.5, 1, '${uuid1}', 1666350500000)`,
        );

        await assertFind(conn, {user: "E"}, {
            "id": "one",
            "s": "first str",
            "n": 42.5,
            "b": true,
            "u": uuid1,
            "d": new Date(1666350500000),
        });
        await assertStoreAndFind(conn, {user: "E"}, {
            "id": "two",
            "s": "žluťoučký kůň",
            "n": -12.345,
            "b": false,
            "u": uuid2,
            "d": new Date(1700000000000),
        });
    });

    await t.case("numbers", async (t) => {
        const conn = await prepare(db,
            {
                name: {user: "E"},
                idType: "string",
                fields: [{name: "num", type: {primitive: "number"}}],
            },
            {
                entityName: {user: "E"},
                tableName: "e",
                idCol: {colName: "id", repr: "stringAsText"},
                fieldCols: [{fieldName: "num", colName: "num", repr: "numberAsDouble"}],
            },
        );
        await db.executeSql("CREATE TABLE e (id TEXT PRIMARY KEY, num REAL NOT NULL)");

        const numbers = [
            0, -0, 1, 42, -42,
            0.12345, 1e-4, 10e6,
            Infinity, -Infinity,
            4294967296, -4294967295, 18446744073709551616, -18446744073709551615,
            1e100, 1e200, 1e300,
            1e-100, 1e-200, 1e-300,
        ];
        for (let i = 0; i < numbers.length; ++i) {
            await assertStoreAndFind(conn, {user: "E"}, {"id": `${i}`, "num": numbers[i]});
        }

        await assertThrows("NaN", async () => {
            await storeWithId(conn, {user: "E"}, {"id": "nan", "num": NaN});
        });
    });

    await t.case("dates", async (t) => {
        const conn = await prepare(db,
            {
                name: {user: "E"},
                idType: "string",
                fields: [{name: "date", type: {primitive: "jsDate"}}],
            },
            {
                entityName: {user: "E"},
                tableName: "e",
                idCol: {colName: "id", repr: "stringAsText"},
                fieldCols: [{fieldName: "date", colName: "date", repr: "jsDateAsDouble"}],
            },
        );
        await db.executeSql("CREATE TABLE e (id TEXT PRIMARY KEY, date REAL NOT NULL)");

        const dates = [
            new Date("2022"),
            new Date("2022-06-15"),
            new Date("2022-06-15T09:13:56"),
            new Date("2022-06-15T09:13:56.1234"),
            new Date(0),
            new Date(1e12),
            new Date(1e14),
            new Date(-1e12),
            new Date(-1e14),
        ];
        for (let i = 0; i < dates.length; ++i) {
            await assertStoreAndFind(conn, {user: "E"}, {"id": `${i}`, "date": dates[i]});
        }

        await assertThrows("NaN", async () => {
            await storeWithId(conn, {user: "E"}, {"id": "nan", "date": new Date(NaN)});
        });
    });

    await t.case("optionals", async (t) => {
        const conn = await prepare(db,
            {
                name: {user: "E"},
                idType: "string",
                fields: [{name: "str", type: {optional: {primitive: "string"}}, optional: true}],
            },
            {
                entityName: {user: "E"},
                tableName: "e",
                idCol: {colName: "id", repr: "stringAsText"},
                fieldCols: [{fieldName: "str", colName: "str", repr: "stringAsText", nullable: true}],
            },
        );
        await db.executeSql("CREATE TABLE e (id TEXT PRIMARY KEY, str TEXT)");

        await assertStoreAndFind(conn, {user: "E"}, {"id": "one", "str": "donkey"});
        await assertStoreAndFind(conn, {user: "E"}, {"id": "two", "str": undefined});
        await assertStoreAndFind(conn, {user: "E"}, {"id": "three"}, {"id": "three", "str": undefined});
    });

    await t.case("arrays of strings", async (t) => {
        const conn = await prepare(db,
            {
                name: {user: "E"},
                idType: "string",
                fields: [{name: "array", type: {array: {primitive: "string"}}}],
            },
            {
                entityName: {user: "E"},
                tableName: "e",
                idCol: {colName: "id", repr: "stringAsText"},
                fieldCols: [{fieldName: "array", colName: "array", repr: "asJsonText"}],
            },
        );

        await db.executeSql(
            "CREATE TABLE e (id TEXT PRIMARY KEY, array TEXT NOT NULL)",
            `INSERT INTO e VALUES ('zero', '[]'), ('one', '["uno"]'), ('two', '["eins", "zwei"]')`,
        );

        await assertFind(conn, {user: "E"}, {"id": "zero", "array": []});
        await assertFind(conn, {user: "E"}, {"id": "one", "array": ["uno"]});
        await assertFind(conn, {user: "E"}, {"id": "two", "array": ["eins", "zwei"]});
        await assertStoreAndFind(conn, {user: "E"}, {"id": "three", "array": ["jedna", "dva", "tři"]});
        await assertStoreAndFind(conn, {user: "E"}, {"id": "four", "array": ["\u{1f98d}"]});
    });

    await t.case("arrays of numbers", async (t) => {
        const conn = await prepare(db,
            {
                name: {user: "E"},
                idType: "string",
                fields: [{name: "array", type: {array: {primitive: "number"}}}],
            },
            {
                entityName: {user: "E"},
                tableName: "e",
                idCol: {colName: "id", repr: "stringAsText"},
                fieldCols: [{fieldName: "array", colName: "array", repr: "asJsonText"}],
            },
        );

        await db.executeSql("CREATE TABLE e (id TEXT PRIMARY KEY, array TEXT NOT NULL)");

        const arrays = [
            [],
            [1],
            [-42],
            [1, 2, 3, 4, 5],
            [0.12345, 1e-4, 10e6],
            [-0, 0],
            [Infinity, -Infinity],
            [4294967296, -4294967295, 18446744073709551616, -18446744073709551615],
            [1e100, 1e200, 1e300],
            [1e-100, 1e-200, 1e-300],
        ];
        for (let i = 0; i < arrays.length; ++i) {
            await assertStoreAndFind(conn, {user: "E"}, {"id": `${i}`, "array": arrays[i]});
        }

        await assertThrows("NaN", async () => {
            await storeWithId(conn, {user: "E"}, {"id": "nan", "array": [1, 2, NaN]});
        });
    });

    await t.case("objects", async (t) => {
        //  class E extends Entity {
        //      id: string,
        //      person: Person,
        //  }
        //
        //  type Person = {
        //      name: string,
        //      birth?: Date,
        //      addresses: Address[],
        //  }
        //
        //  type Address = {
        //      lines: string[],
        //      coords?: {lat: number, lng: number},
        //  }
        const schema = {
            entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "person", type: {typedef: {module: "", name: "Person"}}},
                ],
            }],
            typedefs: [
                [{module: "", name: "Person"}, {object: {fields: [
                    {name: "name", type: {primitive: "string"}},
                    {name: "birth", type: {optional: {primitive: "jsDate"}}, optional: true},
                    {name: "addresses", type: {array: {typedef: {module: "", name: "Address"}}}},
                ]}}],
                [{module: "", name: "Address"}, {object: {fields: [
                    {name: "lines", type: {array: {primitive: "string"}}},
                    {name: "coords", optional: true, type: {optional: {object: {fields: [
                        {name: "lat", type: {primitive: "number"}},
                        {name: "lng", type: {primitive: "number"}},
                    ]}}}},
                ]}}],
            ],
        };
        const layout = {
            entityTables: [{
                entityName: {user: "E"},
                tableName: "e",
                idCol: {colName: "id", repr: "stringAsText"},
                fieldCols: [
                    {fieldName: "person", colName: "person", repr: "asJsonText"},
                ],
            }],
            schema,
        };
        await db.executeSql("CREATE TABLE e (id TEXT PRIMARY KEY, person TEXT NOT NULL)");
        const conn = (await Conn.connect(db, layout)).closeWith(db);

        await assertStoreAndFind(conn, {user: "E"},
            {"id": "one", "person": {
                "name": "Jules Verne",
                "birth": new Date("1828-02-08"),
                "addresses": [
                    {
                        "lines": ["La Madeleine cemetery", "Amiens", "France"],
                        "coords": {"lat": 49.913889, "lng": 2.283611},
                    },
                ],
            }},
        );

        await assertStoreAndFind(conn, {user: "E"},
            {"id": "two", "person": {
                "name": "Jules Verne",
                "addresses": [],
            }},
            {"id": "two", "person": {
                "name": "Jules Verne",
                "birth": undefined,
                "addresses": [],
            }},
        );

        await assertStoreAndFind(conn, {user: "E"},
            {"id": "three", "person": {
                "name": "Jules Verne",
                "addresses": [
                    {"lines": ["24 Rue de l'Ancienne-Comédie", "Paris", "France"]},
                    {"lines": ["44 Boulevard Longueville", "Amiens", "France"]},
                ],
            }},
            {"id": "three", "person": {
                "name": "Jules Verne",
                "birth": undefined,
                "addresses": [
                    {
                        "lines": ["24 Rue de l'Ancienne-Comédie", "Paris", "France"],
                        "coords": undefined,
                    },
                    {
                        "lines": ["44 Boulevard Longueville", "Amiens", "France"],
                        "coords": undefined,
                    },
                ],
            }},
        );

    });

    await t.context("uuid", async (t) => {
        async function test(uuidRepr, uuidSqlType) {
            const conn = await prepare(db,
                {
                    name: {user: "E"},
                    idType: "string",
                    fields: [{name: "uuid", type: {primitive: "uuid"}}],
                },
                {
                    entityName: {user: "E"},
                    tableName: "e",
                    idCol: {colName: "id", repr: "stringAsText"},
                    fieldCols: [{fieldName: "uuid", colName: "uuid", repr: uuidRepr}],
                },
            );
            await db.executeSql(`CREATE TABLE e (id TEXT PRIMARY KEY, uuid ${uuidSqlType} NOT NULL)`);

            await assertStoreAndFind(conn, {user: "E"},
                {"id": "one", "uuid": "fe963c92-ee6c-4961-be1a-77fb19cb09f9"});
            await assertThrows("UUID", async () => {
                await storeWithId(conn, {user: "E"}, {"id": "bad", "uuid": "invalid-uuid"});
            });
            await assertThrows("UUID", async () => {
                await storeWithId(conn, {user: "E"}, {"id": "bad", "uuid": "ec84be5dac384de3a8b4252521abcd56"});
            });
            await assertThrows("UUID", async () => {
                await storeWithId(conn, {user: "E"}, {"id": "bad", "uuid": "FE963C92-EE6C-4961-BE1A-77FB19CB09F9"});
            });
        }

        await t.case("as field", () => test("uuidAsText", "TEXT"));
        await t.case("as json", () => test("asJsonText", "TEXT"));
    });

    await t.context("references", async (t) => {
        async function test(refKind, refType, refRepr, refSqlType, refValues) {
            const schema = {entities: [
                {
                    name: {user: "Post"},
                    idType: refType,
                    fields: [],
                },
                {
                    name: {user: "Comment"},
                    idType: "string",
                    fields: [
                        {name: "post", type: {ref: [{user: "Post"}, refKind]}},
                    ],
                },
            ]};
            const layout = {
                entityTables: [
                    {
                        entityName: {user: "Post"},
                        tableName: "post",
                        idCol: {colName: "id", repr: refRepr},
                        fieldCols: [],
                    },
                    {
                        entityName: {user: "Comment"},
                        tableName: "comment",
                        idCol: {colName: "id", repr: "stringAsText"},
                        fieldCols: [
                            {fieldName: "post", colName: "post_id", repr: refRepr},
                        ],
                    }
                ],
                schema,
            };
            await db.executeSql(
                `CREATE TABLE post (id ${refSqlType} PRIMARY KEY)`,
                `CREATE TABLE comment (id TEXT PRIMARY KEY, post_id ${refSqlType} NOT NULL)`,
            );
            const conn = (await Conn.connect(db, layout)).closeWith(db);

            for (let i = 0; i < refValues.length; ++i) {
                await assertStoreAndFind(conn, {user: "Comment"},
                    {"id": `${i}`, "post": refValues[i]});
            }
        }

        for (const refKind of ["id", "eager"]) {
            const stringValues = ["", "one", "žluťoučký kůň"];
            await t.case(`string ${refKind}`, () =>
                test(refKind, "string", "stringAsText", "TEXT", stringValues));

            const uuidValues = [uuid1, uuid2];
            await t.case(`uuid ${refKind}`, () =>
                test(refKind, "uuid", "uuidAsText", "TEXT", uuidValues));
        }
    });
});
