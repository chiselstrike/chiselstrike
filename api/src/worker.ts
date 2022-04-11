// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import * as Chisel from "./chisel.ts";
(globalThis as unknown as { Chisel: unknown }).Chisel = Chisel;

async function handleMsg(func: () => unknown) {
    let err = undefined;
    let value = undefined;
    try {
        value = await func();
    } catch (e) {
        err = e;
    }
    postMessage({ value, err });
}

function initWorker(id: number) {
    handleMsg(() => {
        Deno.core.opSync("op_chisel_init_worker", id);
    });
}

function readWorkerChannel() {
    handleMsg(() => {
        return Deno.core.opAsync("op_chisel_read_worker_channel");
    });
}

onmessage = function (e) {
    const d = e.data;
    switch (d.cmd) {
        case "readWorkerChannel":
            readWorkerChannel();
            break;
        case "initWorker":
            initWorker(d.id);
            break;
        default:
            throw new Error("unknown command");
    }
};
