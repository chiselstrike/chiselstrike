// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

/// <reference types="./lib.deno_core.d.ts" />
/// <reference lib="dom" />
/// <reference lib="deno.unstable" />

const endpointWorker = new Worker("file:///worker.js", {
    type: "module",
    name: "endpointWorker",
    deno: {
        namespace: true,
    },
});
type Resolver = {
    resolve: (value: unknown) => void;
    reject: (err: Error) => void;
    msg: unknown;
};
const resolvers: Resolver[] = [];
endpointWorker.onmessageerror = function (e) {
    throw e;
};
endpointWorker.onerror = function (e) {
    throw e;
};
endpointWorker.onmessage = function (event) {
    const resolver = resolvers.shift()!;
    const d = event.data;
    const e = d.err;
    if (e) {
        resolver.reject(e);
    } else {
        resolver.resolve(d.value);
    }
};

async function toWorker(msg: unknown) {
    const p = new Promise((resolve, reject) => {
        resolvers.push({ resolve, reject, msg });
    });
    // Each worker should handle a single request at a time, so we
    // only post a message if the worker is not currently
    // busy. Otherwise we leave it scheduled and know it will be
    // posted once the preceding messages are answered.
    if (resolvers.length == 1) {
        endpointWorker.postMessage(resolvers[0].msg);
    }
    try {
        return await p;
    } finally {
        // If a message was scheduled while the worker was busy, post
        // it now.
        if (resolvers.length != 0) {
            endpointWorker.postMessage(resolvers[0].msg);
        }
    }
}

export async function initWorker(id: number) {
    await toWorker({ cmd: "initWorker", id });
}

export async function readWorkerChannel() {
    await toWorker({ cmd: "readWorkerChannel" });
}

export async function importEndpoint(
    path: string,
    apiVersion: string,
    version: number,
) {
    await toWorker({
        cmd: "importEndpoint",
        path,
        apiVersion,
        version,
    });
}

export async function activateEndpoint(path: string) {
    await toWorker({
        cmd: "activateEndpoint",
        path,
    });
}

export async function callHandler(
    userid: string | undefined,
    path: string,
    apiVersion: string,
    url: string,
    method: string,
    headers: HeadersInit,
    rid?: number,
) {
    let chunks = undefined;
    if (rid) {
        chunks = [];
        for (;;) {
            const chunk = await Deno.core.opAsync("op_chisel_read_body", rid);
            if (chunk) {
                chunks.push(chunk);
            } else {
                Deno.core.opSync("op_close", rid);
                break;
            }
        }
    }

    const res = await toWorker({
        cmd: "callHandler",
        userid,
        path,
        apiVersion,
        url,
        method,
        headers,
        chunks,
    }) as { body?: number; status: number; headers: number };
    const body = res.body;
    const read = body
        ? function () {
            const ret = res.body;
            res.body = undefined;
            return ret;
        }
        : undefined;
    return {
        "status": res.status,
        "headers": res.headers,
        "read": read,
    };
}
