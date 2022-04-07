// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import * as Chisel from "./chisel.ts";
(globalThis as unknown as { Chisel: unknown }).Chisel = Chisel;

// Hack to pretend we are not in a web worker. On workers 'window'
// doesn't exist, but globalThis does. They are not exactly the same,
// so we need to force typescript to accept this.
globalThis.window = globalThis as unknown as (Window & typeof globalThis);

type requestHandler = (req: Request) => Promise<Response>;
// Handlers that have been compiled but are not yet serving
// requests. The function activateEndpoint moves handler from
// nextHandlers to handlers.
const nextHandlers: Record<string, requestHandler> = {};
// A map from paths to functions that handle requests for that path.
const handlers: Record<string, requestHandler> = {};

const requestContext = Chisel.requestContext;
const ChiselRequest = Chisel.ChiselRequest;
const loggedInUser = Chisel.loggedInUser;

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

function importEndpoint(
    path: string,
    apiVersion: string,
    version: number,
) {
    handleMsg(() => {
        return importEndpointImpl(path, apiVersion, version);
    });
}

async function importEndpointImpl(
    path: string,
    apiVersion: string,
    version: number,
) {
    requestContext.path = path;
    path = "/" + apiVersion + path;

    // Modules are never unloaded, so we need to create an unique
    // path. This will not be a problem once we publish the entire app
    // at once, since then we can create a new isolate for it.
    const url = `file:///${path}.js?ver=${version}`;
    const mod = await import(url);
    const handler = mod.default;
    if (typeof handler !== "function") {
        throw new Error(
            "expected type `v8::data::Function`, got `v8::data::Value`",
        );
    }
    nextHandlers[path] = handler;
}

function activateEndpoint(path: string) {
    handleMsg(() => {
        handlers[path] = nextHandlers[path];
        delete nextHandlers[path];
    });
}

async function rollback_on_failure<T>(func: () => Promise<T>): Promise<T> {
    try {
        return await func();
    } catch (e) {
        Deno.core.opSync("op_chisel_rollback_transaction");
        throw e;
    }
}

function concat(arrays: Uint8Array[]): Uint8Array {
    let length = 0;
    for (const a of arrays) {
        length += a.length;
    }
    const ret = new Uint8Array(length);
    let i = 0;
    for (const a of arrays) {
        ret.set(a, i);
        i += a.length;
    }
    return ret;
}

async function callHandlerImpl(
    userid: string | undefined,
    path: string,
    apiVersion: string,
    url: string,
    method: string,
    headers: HeadersInit,
    chunks: Uint8Array[],
) {
    requestContext.method = method;
    requestContext.apiVersion = apiVersion;
    requestContext.path = path;
    requestContext.userId = userid;

    // FIXME: maybe defer creating the transaction until we need one, to avoid doing it for
    // endpoints that don't do any data access. For now, because we always create it above,
    // it should be safe to unwrap.
    await Deno.core.opAsync("op_chisel_create_transaction");

    const init: { method: string; headers: HeadersInit; body?: Uint8Array } = {
        method,
        headers,
    };
    if (chunks !== undefined) {
        init.body = concat(chunks);
    }
    const fullPath = "/" + apiVersion + path;
    const pathParams = new URL(url).pathname.replace(
        /\/+/g,
        "/",
    ).replace(/\/$/, "").substring(fullPath.length + 1);
    const user = await loggedInUser();
    const req = new ChiselRequest(
        url,
        init,
        apiVersion,
        path,
        pathParams,
        user,
    );

    const res = await handlers[fullPath](req);
    const resHeaders = [];
    for (const h of res.headers) {
        resHeaders.push(h);
    }
    const reader = res.body?.getReader();
    let body = undefined;
    if (reader) {
        const arrays = [];
        for (;;) {
            const v = await reader.read();
            if (v.done) {
                break;
            }
            arrays.push(v.value);
        }
        body = concat(arrays);
    }
    const status = res.status;

    const resources = Deno.core.resources();
    for (const k in resources) {
        if (parseInt(k) > 2) {
            Deno.core.opSync("op_close", k);
        }
    }

    await Deno.core.opAsync("op_chisel_commit_transaction");
    return { body, status, headers: resHeaders };
}

function callHandler(
    userid: string | undefined,
    path: string,
    apiVersion: string,
    url: string,
    method: string,
    headers: HeadersInit,
    chunks: Uint8Array[],
) {
    handleMsg(() => {
        return rollback_on_failure(() => {
            return callHandlerImpl(
                userid,
                path,
                apiVersion,
                url,
                method,
                headers,
                chunks,
            );
        });
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
        case "importEndpoint":
            importEndpoint(d.path, d.apiVersion, d.version);
            break;
        case "activateEndpoint":
            activateEndpoint(d.path);
            break;
        case "callHandler":
            callHandler(
                d.userid,
                d.path,
                d.apiVersion,
                d.url,
                d.method,
                d.headers,
                d.chunks,
            );
            break;
        default:
            throw new Error("unknown command");
    }
};
