"use strict";

class Resource {
    constructor(rid) {
        this.rid = rid;
        this.closeOnClose = [];
    }

    close() {
        for (const otherRes of this.closeOnClose) {
            otherRes.close();
        }
        Deno.core.close(this.rid);
        this.rid = null;
    }

    closeWith(somethingElse) {
        somethingElse.closeOnClose.push(this);
        return this;
    }
}

class Db extends Resource {
    static async create() {
        return new Db(await Deno.core.opAsync("op_test_create_db"));
    }

    async migrate(oldLayout, newSchema) {
        return await Deno.core.opAsync("op_test_migrate", this.rid, oldLayout, newSchema);
    }

    async executeSql(...sqls) {
        for (const sql of sqls) {
            await Deno.core.opAsync("op_test_execute_sql", this.rid, sql);
        }
    }

    async fetchSql(sql) {
        return await Deno.core.opAsync("op_test_fetch_sql", this.rid, sql);
    }
}

class Conn extends Resource {
    static async connect(db, layout, callback) {
        const conn = new Conn(Deno.core.opSync("op_test_connect", db.rid, layout));
        if (callback) {
            await callback(conn);
            conn.close();
        } else {
            return conn;
        }
    }

    async begin(callback) {
        const ctx = new DataCtx(await Deno.core.opAsync("op_datastore_begin", this.rid));
        if (callback) {
            await callback(ctx);
            await ctx.commit();
        } else {
            return ctx;
        }
    }
}

class Query extends Resource {
    static findById(conn, entityName) {
        const rid = Deno.core.opSync("op_datastore_query_find_by_id", conn.rid, entityName);
        return new Query(rid);
    }

    static storeWithId(conn, entityName) {
        const rid = Deno.core.opSync("op_datastore_query_store_with_id", conn.rid, entityName);
        return new Query(rid);
    }

    startFetch(arg, callback) {
        const stream = new FetchStream(Deno.core.opSync("op_datastore_fetch_start", this.rid, arg));
        if (callback) {
            return callback(stream).then(() => stream.close());
        } else {
            return future;
        }
    }

    startExecute(arg, callback) {
        const future = new ExecuteFuture(Deno.core.opSync("op_datastore_execute_start", this.rid, arg));
        if (callback) {
            return callback(future).then(() => future.close());
        } else {
            return future;
        }
    }

    async fetch(ctx, arg) {
        const values = [];
        await this.startFetch(arg, async (stream) => {
            while (await stream.fetch(ctx)) {
                values.push(stream.read());
            }
        });
        return values;
    }

    async fetchOne(ctx, arg) {
        const values = await this.fetch(ctx, arg);
        if (values.length != 1) {
            throw new Error(`expected exactly one result, but got ${values.length}`);
        }
        return values[0];
    }

    async execute(ctx, arg) {
        await this.startExecute(arg, (fut) => fut.execute(ctx));
    }
}

class DataCtx extends Resource {
    async commit() {
        await Deno.core.opAsync("op_datastore_commit", this.rid);
        this.rid = null;
    }

    async rollback() {
        await Deno.core.opAsync("op_datastore_rollback", this.rid);
        this.rid = null;
    }
}

class FetchStream extends Resource {
    async fetch(ctx) {
        return await Deno.core.opAsync("op_datastore_fetch", ctx.rid, this.rid);
    }

    read() {
        return Deno.core.opSync("op_datastore_fetch_read", this.rid);
    }
}

class ExecuteFuture extends Resource {
    async execute(ctx) {
        await Deno.core.opAsync("op_datastore_execute", ctx.rid, this.rid);
    }

    rowsAffected() {
        return Deno.core.opSync("op_datastore_execute_rows_affected", this.rid);
    }
}

async function findById(conn, entityName, id) {
    const query = Query.findById(conn, entityName);
    const ctx = await conn.begin();
    const obj = await query.fetchOne(ctx, id);
    ctx.close();
    query.close();
    return obj;
}

async function storeWithId(conn, entityName, obj) {
    const query = Query.storeWithId(conn, entityName);
    const ctx = await conn.begin();
    await query.execute(ctx, obj);
    await ctx.commit();
    query.close();
}

async function assertFind(conn, entityName, findObj) {
    assertJsonEq(await findById(conn, entityName, findObj["id"]), findObj);
}

async function assertStoreAndFind(conn, entityName, storeObj, findObj = storeObj) {
    await storeWithId(conn, entityName, storeObj);
    assertJsonEq(await findById(conn, entityName, storeObj["id"]), findObj);
}
