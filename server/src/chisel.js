// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

import { Table, table } from "./api.ts";

var Chisel = {}

Chisel.api = {}
Chisel.api.Table = Table;
Chisel.api.table = table;

Chisel.buildReadableStreamForBody = function(rid) {
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
        }
    });
}

Chisel.store = async function(type_name, content) {
    await Deno.core.opAsync("chisel_store", {name: type_name, value: content});
}

function create_result_iterator(rid) {
    return {
        [Symbol.asyncIterator]() {
            return {
                async next() {
                    const value = await Deno.core.opAsync("chisel_query_next", rid);
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
                }
            }
        }
    }
}

Chisel.find_all = async function(type_name) {
    const rid = await Deno.core.opAsync("chisel_query_create", {
        type_name: type_name
    });
    return create_result_iterator(rid);
}

Chisel.find_all_by = async function(type_name, field_name, value) {
    const rid = await Deno.core.opAsync("chisel_query_create", {
        type_name: type_name,
        field_name: field_name,
        value: value
    });
    return create_result_iterator(rid);
}


Chisel.json = function(body, status = 200) {
    return new Response(JSON.stringify(body), {
        status: status,
        headers: [
            ["content-type", "application/json"]
        ]
    })
}

globalThis.Chisel = Chisel;
