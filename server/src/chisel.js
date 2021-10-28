// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

Chisel = {}
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

Chisel.find_all = async function(type_name) {
    const rid = await Deno.core.opAsync("chisel_query_create", type_name);
    let result = {
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
    };
    return result;
}
