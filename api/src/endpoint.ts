// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

/// <reference lib="deno.core" />
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

type BodyState = {
    parts: { value?: Uint8Array; err?: Error }[];
    done: boolean;
    resolve?: () => void;
};

const bodyParts: Record<number, BodyState> = {};
let bodyDone = false;
let resolveBody: (() => void) | undefined = undefined;
endpointWorker.onmessage = function (event) {
    const { msg, id, value, err } = event.data;
    if (msg == "body") {
        const state = bodyParts[id];
        if (state?.resolve !== undefined) {
            state.resolve();
            state.resolve = undefined;
        }
        if ((err !== undefined || value !== undefined) && state !== undefined) {
            state.parts.push({ value, err });
        }
        if (err !== undefined || value === undefined) {
            if (state !== undefined) {
                state.done = true;
            }
            bodyDone = true;
            if (resolveBody !== undefined) {
                resolveBody();
                resolveBody = undefined;
            }
        }
    } else {
        const resolver = resolvers[0];
        if (err) {
            resolver.reject(err);
        } else {
            resolver.resolve(value);
        }
    }
};

function sendMsg(msg: unknown) {
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
    return p;
}

async function toWorker(msg: unknown) {
    try {
        return await sendMsg(msg);
    } finally {
        endMsgProcessing();
    }
}

function endMsgProcessing() {
    resolvers.shift();
    // If a message was scheduled while the worker was busy, post
    // it now.
    if (resolvers.length != 0) {
        endpointWorker.postMessage(resolvers[0].msg);
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

export function endOfRequest(id: number) {
    endpointWorker.postMessage({ cmd: "endOfRequest", id });
    delete bodyParts[id];
}

function clear() {
    // Clear for the next request
    bodyDone = false;
    endMsgProcessing();
}

export async function callHandler(
    path: string,
    apiVersion: string,
    id: number,
) {
    bodyParts[id] = { parts: [], done: false };

    let res;
    try {
        res = await sendMsg({
            cmd: "callHandler",
            path,
            apiVersion,
            id,
        }) as { status: number; headers: number };
    } catch (e) {
        clear();
        throw e;
    }

    let bodyDonePromise: Promise<void>;
    if (!bodyDone) {
        bodyDonePromise = new Promise<void>((resolve) => {
            resolveBody = resolve;
        });
        // Don't await for the new promise. We want a background task
        // to run clear once we have received the full body.
        bodyDonePromise.then(clear);
    } else {
        clear();
    }

    // The read function is called repeatedly until it returns
    // undefined.
    const read = async function () {
        const state = bodyParts[id];

        if (state.parts.length === 0 && !state.done) {
            await new Promise<void>((resolve) => {
                state.resolve = resolve;
            });
        }

        const elem = state.parts.shift();
        const err = elem?.err;
        if (err !== undefined) {
            throw err;
        }
        return elem?.value;
    };
    return {
        "status": res.status,
        "headers": res.headers,
        "read": read,
    };
}
