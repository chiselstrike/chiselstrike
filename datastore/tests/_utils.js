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

    closeWith(otherRes) {
        otherRes.closeOnClose.push(this);
        return this;
    }
}

class Conn extends Resource {
    static async connect(layout, callback) {
        const conn = new Conn(await Deno.core.opAsync("op_test_connect", layout));
        if (callback) {
            await callback(conn);
            conn.close();
        } else {
            return conn;
        }
    }

    async executeSql(...sqls) {
        for (const sql of sqls) {
            await Deno.core.opAsync("op_test_execute_sql", this.rid, sql);
        }
    }

    async fetchSql(sql) {
        return await Deno.core.opAsync("op_test_fetch_sql", this.rid, sql);
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

function assertFail(message) {
    if (typeof message === "function") {
        message = message();
    }
    throw new Error(message);
}

function assert(value, message) {
    if (!value) {
        assertFail(message ?? "value is not true");
    }
}

function assertEq(left, right, message) {
    if (left !== right) {
        assertFail(message ?? `${left} !== ${right}`);
    }
}

function assertJsonEq(left, right, message) {
    if (!jsonEq(left, right)) {
        assertFail(message ?? `${JSON.stringify(left)} does not equal ${JSON.stringify(right)}`);
    }
}

function jsonEq(left, right) {
    if (typeof left !== typeof right) {
        return false;
    } else if (left === right) {
        return true;
    } else if (typeof left === "object") {
        for (const key in left) {
            if (!(key in right) || !jsonEq(left[key], right[key])) {
                return false;
            }
        }
        for (const key in right) {
            if (!(key in left)) {
                return false;
            }
        }
        return true;
    } else if (Array.isArray(left) && Array.isArray(right)) {
        if (left.length !== right.length) {
            return false;
        }
        for (let i = 0; i < left.length; ++i) {
            if (!jsonEq(left[i], right[i])) {
                return false;
            }
        }
        return true;
    } else {
        return false;
    }
}

function println(val) {
    Deno.core.opSync("op_test_println", val);
}
