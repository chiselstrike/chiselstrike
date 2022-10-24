"use strict";
t.context("migrate", async (t) => {
    const db = (await Db.create()).closeWith(t);

    async function migrate(oldLayout, newSchema) {
        const newLayout = await db.migrate(oldLayout, newSchema);
        return (await Conn.connect(db, newLayout)).closeWith(t);
    }

    const uuid1 = "4c2a2753-2cbf-47b5-bee5-784939c677d3";
    const uuid2 = "b7c29bb4-479e-4a40-8a15-05d2efab328f";

    await t.context("from empty", async (t) => {
        const oldLayout = {entityTables: [], schema: {entities: []}};

        await t.case("add simple entity", async (t) => {
            const newSchema = {entities: [{
                name: {user: "Book"},
                idType: "string",
                fields: [
                    {name: "title", type: {primitive: "string"}},
                    {name: "pageCount", type: {optional: {primitive: "number"}}, optional: true},
                ],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertStoreAndFind(conn, {user: "Book"},
                {id: "pap", title: "Pride and Prejudice"},
                {id: "pap", title: "Pride and Prejudice", pageCount: undefined},
            );
            await assertStoreAndFind(conn, {user: "Book"},
                {id: "as", title: "Atlas Shrugged", pageCount: 1000}
            );
        });

        await t.case("add entity with primitives", async (t) => {
            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "s", type: {primitive: "string"}},
                    {name: "n", type: {primitive: "number"}},
                    {name: "b", type: {primitive: "boolean"}},
                    {name: "u", type: {primitive: "uuid"}},
                    {name: "d", type: {primitive: "jsDate"}},
                ],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertStoreAndFind(conn, {user: "E"}, {
                "id": "two",
                "s": "žluťoučký kůň",
                "n": -12.345,
                "b": false,
                "u": uuid1,
                "d": new Date(1700000000000),
            });
        });

        await t.case("add entity with collections", async (t) => {
            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "uuid",
                fields: [
                    {name: "array", type: {array: {primitive: "string"}}},
                    {name: "obj", type: {object: {fields: [
                        {name: "a", type: {primitive: "number"}},
                        {name: "b", type: {optional: {primitive: "boolean"}}},
                    ]}}},
                ],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertStoreAndFind(conn, {user: "E"}, {
                "id": uuid1,
                "array": ["quick", "brown", "fox"],
                "obj": {"a": 0.25, "b": false},
            });
        });

        await t.case("add two entities", async (t) => {
            const newSchema = {entities: [
                {
                    name: {user: "Commit"},
                    idType: "string",
                    fields: [
                        {name: "message", type: {optional: {primitive: "string"}}},
                        {name: "author", type: {ref: [{user: "Person"}, "id"]}},
                        {name: "committer", type: {ref: [{user: "Person"}, "id"]}},
                    ],
                },
                {
                    name: {user: "Person"},
                    idType: "string",
                    fields: [
                        {name: "name", type: {primitive: "string"}},
                    ],
                },
            ]};
            const conn = await migrate(oldLayout, newSchema);

            await assertStoreAndFind(conn, {user: "Person"},
                {"id": "darcy", "name": "Mr. Darcy"});
            await assertStoreAndFind(conn, {user: "Person"},
                {"id": "lizzy", "name": "Elisabeth Bennet"});

            await assertStoreAndFind(conn, {user: "Commit"},
                {
                    "id": "one",
                    "author": "lizzy",
                    "committer": "darcy",
                },
                {
                    "id": "one",
                    "message": undefined,
                    "author": "lizzy",
                    "committer": "darcy",
                },
            );
        });
    });

    await t.context("existing entity", async (t) => {
        await t.case("add optional field", async (t) => {
            const oldSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [],
            }]};
            const oldLayout = {
                entityTables: [{
                    entityName: {user: "E"},
                    tableName: "e",
                    idCol: {colName: "id", repr: "stringAsText"},
                    fieldCols: [],
                }],
                schema: oldSchema,
            };
            await db.executeSql(
                "CREATE TABLE e (id TEXT PRIMARY KEY)",
                "INSERT INTO e VALUES ('one')",
            );

            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "added", type: {optional: {primitive: "number"}}, default: "undefined"},
                ],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertFind(conn, {user: "E"}, {"id": "one", "added": undefined});
            await assertStoreAndFind(conn, {user: "E"}, {"id": "two", "added": 1234});
        });

        await t.case("add required field", async (t) => {
            const oldSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [],
            }]};
            const oldLayout = {
                entityTables: [{
                    entityName: {user: "E"},
                    tableName: "e",
                    idCol: {colName: "id", repr: "stringAsText"},
                    fieldCols: [],
                }],
                schema: oldSchema,
            };
            await db.executeSql(
                "CREATE TABLE e (id TEXT PRIMARY KEY)",
                "INSERT INTO e VALUES ('one')",
            );

            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "addedNum", type: {primitive: "number"}, default: {number: 42}},
                    {name: "addedInf", type: {primitive: "number"}, default: {number: "negInf"}},
                    {name: "addedStr", type: {primitive: "string"}, default: {string: "žluťoučký ' kůň"}},
                ],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertFind(conn, {user: "E"}, {
                "id": "one",
                "addedNum": 42,
                "addedInf": -Infinity,
                "addedStr": "žluťoučký ' kůň",
            });
            await assertStoreAndFind(conn, {user: "E"}, {
                "id": "two",
                "addedNum": 1000,
                "addedInf": 1e6,
                "addedStr": "yellow horse",
            });
        });

        await t.case("remove field", async (t) => {
            const oldSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "str", type: {primitive: "string"}},
                ],
            }]};
            const oldLayout = {
                entityTables: [{
                    entityName: {user: "E"},
                    tableName: "e",
                    idCol: {colName: "id", repr: "stringAsText"},
                    fieldCols: [
                        {fieldName: "str", colName: "str", repr: "stringAsText"},
                    ],
                }],
                schema: oldSchema,
            };
            await db.executeSql(
                "CREATE TABLE e (id TEXT PRIMARY KEY, str TEXT NOT NULL)",
                "INSERT INTO e VALUES ('one', 'quick fox')",
            );

            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertFind(conn, {user: "E"}, {"id": "one"});
            await assertStoreAndFind(conn, {user: "E"}, {"id": "two"});
        });

        await t.case("update primitive field to a supertype", async (t) => {
            const oldSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "u", type: {primitive: "uuid"}},
                ],
            }]};
            const oldLayout = {
                entityTables: [{
                    entityName: {user: "E"},
                    tableName: "e",
                    idCol: {colName: "id", repr: "stringAsText"},
                    fieldCols: [
                        {fieldName: "u", colName: "u", repr: "uuidAsText"},
                    ],
                }],
                schema: oldSchema,
            };
            await db.executeSql(
                "CREATE TABLE e (id TEXT PRIMARY KEY, u TEXT NOT NULL)",
                `INSERT INTO e VALUES ('one', '${uuid1}')`,
            );

            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "u", type: {primitive: "string"}},
                ],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertFind(conn, {user: "E"}, {"id": "one", "u": uuid1});
            await assertStoreAndFind(conn, {user: "E"}, {"id": "two", "u": "fox in a box"});
        });

        await t.case("make a field optional", async (t) => {
            const oldSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "s", type: {primitive: "string"}},
                ],
            }]};
            const oldLayout = {
                entityTables: [{
                    entityName: {user: "E"},
                    tableName: "e",
                    idCol: {colName: "id", repr: "stringAsText"},
                    fieldCols: [
                        {fieldName: "s", colName: "s", repr: "stringAsText"},
                    ],
                }],
                schema: oldSchema,
            };
            await db.executeSql(
                "CREATE TABLE e (id TEXT PRIMARY KEY, s TEXT NOT NULL)",
                `INSERT INTO e VALUES ('one', 'first fox')`,
            );

            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "s", type: {optional: {primitive: "string"}}},
                ],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertFind(conn, {user: "E"}, {"id": "one", "s": "first fox"});
            await assertStoreAndFind(conn, {user: "E"}, {"id": "two", "s": undefined});
        });

        await t.case("update object field to a supertype", async (t) => {
            const oldSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "o", type: {object: {fields: [
                        {name: "x", type: {primitive: "number"}},
                    ]}}},
                ],
            }]};
            const oldLayout = {
                entityTables: [{
                    entityName: {user: "E"},
                    tableName: "e",
                    idCol: {colName: "id", repr: "stringAsText"},
                    fieldCols: [
                        {fieldName: "o", colName: "o", repr: "asJsonText"},
                    ],
                }],
                schema: oldSchema,
            };
            await db.executeSql(
                "CREATE TABLE e (id TEXT PRIMARY KEY, o TEXT NOT NULL)",
                `INSERT INTO e VALUES ('one', '{"x": 1.0}')`,
            );

            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "o", type: {object: {fields: [
                        {name: "x", type: {primitive: "number"}},
                        {name: "y", type: {optional: {primitive: "number"}}, optional: true},
                    ]}}},
                ],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertFind(conn, {user: "E"}, {"id": "one", "o": {"x": 1}});
            await assertStoreAndFind(conn, {user: "E"}, {"id": "two", "o": {"x": 2, "y": 100}});
        });

        await t.case("update id to a supertype", async (t) => {
            const oldSchema = {entities: [{
                name: {user: "E"},
                idType: "uuid",
                fields: [],
            }]};
            const oldLayout = {
                entityTables: [{
                    entityName: {user: "E"},
                    tableName: "e",
                    idCol: {colName: "id", repr: "uuidAsText"},
                    fieldCols: [],
                }],
                schema: oldSchema,
            };
            await db.executeSql(
                "CREATE TABLE e (id TEXT PRIMARY KEY)",
                `INSERT INTO e VALUES ('${uuid1}')`,
            );

            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [],
            }]};
            const conn = await migrate(oldLayout, newSchema);

            await assertFind(conn, {user: "E"}, {"id": uuid1});
            await assertStoreAndFind(conn, {user: "E"}, {"id": "two"});
        });
    });

    await t.context("forbidden migrations", async (t) => {
        async function assertMigrateThrows(oldLayout, newSchema, pattern) {
            await assertThrows(pattern, () => migrate(oldLayout, newSchema));
        }

        const oldSchema = {entities: [{
            name: {user: "E"},
            idType: "string",
            fields: [
                {name: "oldStr", type: {primitive: "string"}},
            ],
        }]};
        const oldLayout = {
            entityTables: [{
                entityName: {user: "E"},
                tableName: "e",
                idCol: {colName: "id", repr: "stringAsText"},
                fieldCols: [
                    {fieldName: "oldStr", colName: "oldStr", repr: "stringAsText"},
                ],
            }],
            schema: oldSchema,
        };

        await t.case("change type of the id to incompatible type", async (t) => {
            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "uuid",
                fields: [
                    {name: "oldStr", type: {primitive: "string"}},
                ],
            }]};
            await assertMigrateThrows(oldLayout, newSchema, "id");
        });

        await t.case("change type of field to incompatible type", async (t) => {
            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "uuid",
                fields: [
                    {name: "oldStr", type: {primitive: "number"}},
                ],
            }]};
            await assertMigrateThrows(oldLayout, newSchema, "id");
        });

        await t.case("add field without default value", async (t) => {
            const newSchema = {entities: [{
                name: {user: "E"},
                idType: "string",
                fields: [
                    {name: "oldStr", type: {primitive: "string"}},
                    {name: "noDefault", type: {primitive: "boolean"}},
                ],
            }]};
            await assertMigrateThrows(oldLayout, newSchema, "default value");
        });
    });
});
