// FIXME: this should be typescript

import * as Chisel from "./chisel.ts";
globalThis.Chisel = Chisel;

// FIXME: This is export just to silence the linter
export async function handleMsg(func) {
    let err = undefined;
    let value = undefined;
    try {
        value = await func();
    } catch (e) {
        err = e;
    }
    postMessage({ value, err });
}

onmessage = function (e) {
    const d = e.data;
    switch (d.cmd) {
        default:
            throw new Error("unknown command");
    }
};
