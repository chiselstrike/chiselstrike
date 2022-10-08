const schema = {
    entities: [
        {
            name: {user: "Book"},
            idType: "Uuid",
            fields: [],
        }
    ],
};

const layout = {
    entityTables: [
        {
            entityName: {user: "Book"},
            tableName: "book",
            idCol: {colName: "id", repr: "UuidAsString"},
            fieldCols: [],
        },
    ],
    schema,
};

(async function test() {
    const connRid = await Deno.core.opAsync("op_test_connect", layout);
    await Deno.core.opAsync("op_test_execute_sql", connRid,
        "CREATE TABLE book (id TEXT PRIMARY KEY)",
    );

    const findQueryRid = Deno.core.opSync("op_datastore_find_by_id_query", connRid, {user: "Book"});
    const storeQueryRid = Deno.core.opSync("op_datastore_store_with_id_query", connRid, {user: "Book"});

    const ctxRid = await Deno.core.opAsync("op_datastore_begin", connRid);

    const execRid = Deno.core.opSync("op_datastore_execute_start", storeQueryRid, {"id": "pride-prejudice"});
    await Deno.core.opAsync("op_datastore_execute", ctxRid, execRid);
    const rows = Deno.core.opSync("op_datastore_execute_rows_affected", execRid);
    Deno.core.opSync("op_test_println", rows);
    Deno.core.close(execRid);

    const fetchRid = Deno.core.opSync("op_datastore_fetch_start", findQueryRid, "pride-prejudice");
    await Deno.core.opAsync("op_datastore_fetch", ctxRid, fetchRid);
    const obj = Deno.core.opSync("op_datastore_fetch_read", fetchRid);
    Deno.core.opSync("op_test_println", obj);
    Deno.core.close(fetchRid);

    await Deno.core.opAsync("op_datastore_commit", ctxRid);

    Deno.core.close(storeQueryRid);
    Deno.core.close(findQueryRid);
    Deno.core.close(connRid);
})()
