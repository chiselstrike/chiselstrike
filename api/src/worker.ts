// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import * as Chisel from "./chisel.ts";
(globalThis as unknown as { Chisel: unknown }).Chisel = Chisel;

// Hack to pretend we are not in a web worker. On workers 'window'
// doesn't exist, but globalThis does. They are not exactly the same,
// so we need to force typescript to accept this.
(globalThis as unknown as { window: unknown }).window = globalThis;

Deno.core.opSync(
    "op_set_promise_reject_callback",
    (type: number, _promise: unknown, reason: unknown) => {
        if (type == 0) { // PromiseRejectWithNoHandler
            // Without this function deno pushes the exception to
            // pending_promise_exceptions, which eventually causes an unlucky
            // user of poll_event_loop to get an error. Since user code can
            // create and reject a promise that lacks a handler, we have to do
            // this. Throwing in here causes deno to at least log the stack.
            throw new Error("Promise rejected without a handler: " + reason);
        }
    },
);

type requestHandler = (req: Request) => Promise<Response>;
// Handlers that have been compiled but are not yet serving
// requests. The function activateEndpoint moves handler from
// nextHandlers to handlers.
const nextHandlers: Record<string, requestHandler> = {};
// A map from paths to functions that handle requests for that path.
const handlers: Record<string, requestHandler> = {};

type eventHandler = (event: Chisel.ChiselEvent) => Promise<void>;
const nextEventHandlers: Record<string, eventHandler> = {};
const eventHandlers: Record<string, eventHandler> = {};

const requestContext = Chisel.requestContext;
const ChiselRequest = Chisel.ChiselRequest;
const loggedInUser = Chisel.loggedInUser;

function sendBodyPart(
    value: Uint8Array | undefined,
    id: number,
    err?: unknown,
) {
    postMessage({ msg: "body", value, err, id });
}

async function handleMsg(func: () => unknown) {
    let err = undefined;
    let value = undefined;
    try {
        value = await func();
    } catch (e) {
        err = e;
    }
    postMessage({ msg: "reply", value, err });
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

type Endpoint = { path: string; apiVersion: string; version: number };

type EventHandler = { path: string; apiVersion: string; version: number };

function importEndpoints(endpoints: [Endpoint], eventHandlers: [EventHandler]) {
    handleMsg(() => {
        return importEndpointsImpl(endpoints, eventHandlers);
    });
}

async function importEndpointsImpl(
    endpoints: [Endpoint],
    eventHandlers: [EventHandler],
) {
    for (const endpoint of endpoints) {
        const { path, apiVersion } = endpoint;

        requestContext.path = path;
        const fullPath = "/" + apiVersion + path;

        // Modules are never unloaded, so we need to create an unique
        // path. This will not be a problem once we publish the entire app
        // at once, since then we can create a new isolate for it.
        const url = `file:///${apiVersion}/endpoints${path}`;
        const mod = await import(url);
        const handler = mod.default;
        if (typeof handler !== "function") {
            throw new Error(
                "expected type `v8::data::Function`, got `v8::data::Value`",
            );
        }
        nextHandlers[fullPath] = handler;
    }
    for (const eventHandler of eventHandlers) {
        const { path, apiVersion } = eventHandler;

        requestContext.path = path;
        const fullPath = "/" + apiVersion + path;

        // Modules are never unloaded, so we need to create an unique
        // path. This will not be a problem once we publish the entire app
        // at once, since then we can create a new isolate for it.
        const url = `file:///${apiVersion}/events${path}`;
        const mod = await import(url);
        const handler = mod.default;
        if (typeof handler !== "function") {
            throw new Error(
                "expected type `v8::data::Function`, got `v8::data::Value`",
            );
        }
        nextEventHandlers[fullPath] = handler;
    }
}

function activateEndpoint(path: string) {
    handleMsg(() => {
        handlers[path] = nextHandlers[path];
        delete nextHandlers[path];
    });
}

function activateEventHandler(path: string) {
    handleMsg(() => {
        eventHandlers[path] = nextEventHandlers[path];
        delete nextEventHandlers[path];
    });
}

async function rollback_on_failure<T>(func: () => Promise<T>): Promise<T> {
    try {
        return await func();
    } catch (e) {
        closeResources();
        Deno.core.opSync("op_chisel_rollback_transaction");
        throw e;
    }
}

function buildReadableStreamForBody(rid: number) {
    return new ReadableStream<Uint8Array>({
        async pull(controller: ReadableStreamDefaultController) {
            const chunk = await Deno.core.opAsync("op_chisel_read_body", rid);
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
}

function closeResources() {
    const resources = Deno.core.resources();
    for (const k in resources) {
        if (parseInt(k) > 2) {
            Deno.core.opSync("op_close", k);
        }
    }
}

async function sendBody(
    reader: ReadableStreamDefaultReader<Uint8Array> | undefined,
    id: number,
) {
    try {
        if (reader !== undefined) {
            for (let i = 0;; i += 1) {
                const v = await reader.read();
                // FIXME: Is this the correct way to yield in async JS?
                if (i % 16 == 0) {
                    await new Promise((resolve) => setTimeout(resolve, 0));
                }
                if (v.done || currentRequestId === undefined) {
                    break;
                }
                sendBodyPart(v.value, id);
            }
        }
        closeResources();
        await Deno.core.opAsync("op_chisel_commit_transaction");

        sendBodyPart(undefined, id);
    } catch (e) {
        closeResources();
        Deno.core.opSync("op_chisel_rollback_transaction");

        sendBodyPart(undefined, id, e);
    }
}

let currentRequestId: number | undefined;
async function callHandlerImpl(
    path: string,
    apiVersion: string,
    id: number,
) {
    currentRequestId = id;
    requestContext.apiVersion = apiVersion;
    requestContext.path = path;

    const start = await Deno.core.opAsync("op_chisel_start_request");
    if (start.Special) {
        sendBodyPart(start.Special.body, id);
        sendBodyPart(undefined, id);
        return start.Special;
    }
    const { userid, url, method, headers, body_rid } = start.Js;
    requestContext.method = method;
    requestContext.userId = userid;
    requestContext.headers = headers;

    // FIXME: maybe defer creating the transaction until we need one, to avoid doing it for
    // endpoints that don't do any data access. For now, because we always create it above,
    // it should be safe to unwrap.
    await Deno.core.opAsync("op_chisel_create_transaction");

    const init: RequestInit = {
        method,
        headers,
    };

    if (body_rid != undefined) {
        const body = buildReadableStreamForBody(body_rid);
        init.body = body;
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

    let res = await handlers[fullPath](req);
    const resHeaders = [];
    // FIXME: we could try to building a ReadableStream from
    // this instead of materializing a full response. Probably
    // a bit faster but this is a lot simpler for now.
    if (res?.constructor.name != "Response") {
        res = Chisel.responseFromJson(res);
    }

    for (const h of res.headers) {
        resHeaders.push(h);
    }
    const reader = res.body?.getReader();

    // Don't wait on sendBody as we want to send the body as a
    // background job.
    sendBody(reader, id);

    const status = res.status;
    return { status, headers: resHeaders };
}

function callHandler(
    path: string,
    apiVersion: string,
    id: number,
) {
    handleMsg(() => {
        return rollback_on_failure(() => {
            return callHandlerImpl(
                path,
                apiVersion,
                id,
            );
        });
    });
}

async function callEventHandlerImpl(
    path: string,
    apiVersion: string,
    key: ArrayBuffer,
    value: ArrayBuffer,
) {
    requestContext.apiVersion = apiVersion;

    await Deno.core.opAsync("op_chisel_start_event_handler");

    await Deno.core.opAsync("op_chisel_create_transaction");

    const fullPath = "/" + apiVersion + path;

    await eventHandlers[fullPath]({ key, value });

    closeResources();

    await Deno.core.opAsync("op_chisel_commit_transaction");
}

function callEventHandler(
    path: string,
    apiVersion: string,
    key: ArrayBuffer,
    value: ArrayBuffer,
) {
    handleMsg(() => {
        return rollback_on_failure(() => {
            return callEventHandlerImpl(
                path,
                apiVersion,
                key,
                value,
            );
        });
    });
}

function endOfRequest(id: number) {
    if (id == currentRequestId) {
        currentRequestId = undefined;
    }
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
        case "importEndpoints":
            importEndpoints(d.endpoints, d.eventHandlers);
            break;
        case "activateEndpoint":
            activateEndpoint(d.path);
            break;
        case "activateEventHandler":
            activateEventHandler(d.path);
            break;
        case "callHandler":
            callHandler(
                d.path,
                d.apiVersion,
                d.id,
            );
            break;
        case "callEventHandler":
            callEventHandler(
                d.path,
                d.apiVersion,
                d.key,
                d.value,
            );
            break;
        case "endOfRequest":
            endOfRequest(d.id);
            break;
        default:
            throw new Error("unknown command");
    }
};
