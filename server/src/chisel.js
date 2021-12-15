// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

import { Table, table } from "./api.ts";

const Chisel = {};

Chisel.collections = {};

Chisel.api = {};
Chisel.api.Table = Table;
Chisel.api.table = table;

Chisel.buildReadableStreamForBody = function (rid) {
    return new ReadableStream({
        async pull(controller) {
            const chunk = await Deno.core.opAsync("chisel_read_body", rid);
            if (chunk) {
                controller.enqueue(chunk);
            } else {
                controller.close();
                Deno.core.opSync("op_close", rid);
            }
        },
        cancel() {
            Deno.core.opSync("op_close", rid);
        },
    });
};

Chisel.store = async function (typeName, content) {
    await Deno.core.opAsync("chisel_store", { name: typeName, value: content });
};

function createResultIterator(rid) {
    return {
        [Symbol.asyncIterator]() {
            return {
                async next() {
                    const value = await Deno.core.opAsync(
                        "chisel_relational_query_next",
                        rid,
                    );
                    if (value) {
                        return { value: value, done: false };
                    } else {
                        Deno.core.opSync("op_close", rid);
                        return { done: true };
                    }
                },
                return() {
                    Deno.core.opSync("op_close", rid);
                    return { done: true };
                },
            };
        },
    };
}

Chisel.query = function (relation) {
    const rid = Deno.core.opSync(
        "chisel_relational_query_create",
        relation.inner,
    );
    return createResultIterator(rid);
};

Chisel.json = function (body, status = 200) {
    return new Response(JSON.stringify(body), {
        status: status,
        headers: [
            ["content-type", "application/json"],
        ],
    });
};

globalThis.Chisel = Chisel;
