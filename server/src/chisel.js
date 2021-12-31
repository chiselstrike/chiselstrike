// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

import { ChiselEntity, ChiselIterator, chiselIterator } from "./api.ts";

const Chisel = {};

Chisel.api = {};
Chisel.api.ChiselIterator = ChiselIterator;
Chisel.api.chiselIterator = chiselIterator;
globalThis.ChiselEntity = ChiselEntity;

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

Chisel.save = async function (typeName, content) {
    return await Deno.core.opAsync("chisel_store", {
        name: typeName,
        value: content,
    });
};

/**
 * NOTE! This function is marked for deprecation in favor of `Chisel.save()`.
 */
Chisel.store = Chisel.save;

Chisel.json = function (body, status = 200) {
    return new Response(JSON.stringify(body), {
        status: status,
        headers: [
            ["content-type", "application/json"],
        ],
    });
};

globalThis.Chisel = Chisel;
