// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

/// <reference types="./lib.deno_core.d.ts" />
/// <reference lib="dom" />
/// <reference lib="dom.iterable" />

// Handlers that have been compiled but are not yet serving requests.
type requestHandler = (req: Request) => Promise<Response>;
const nextHandlers: Record<string, requestHandler> = {};
const handlers: Record<string, requestHandler> = {};

import { ChiselRequest, loggedInUser, requestContext } from "./chisel.ts";

function buildReadableStreamForBody(rid: number) {
    return new ReadableStream<string>({
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

export async function importEndpoint(
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

export function activateEndpoint(path: string) {
    handlers[path] = nextHandlers[path];
    delete nextHandlers[path];
}

async function rollback_on_failure<T>(func: () => Promise<T>): Promise<T> {
    try {
        return await func();
    } catch (e) {
        Deno.core.opSync("op_chisel_rollback_transaction");
        throw e;
    }
}

export function callHandler(
    userid: string | undefined,
    path: string,
    apiVersion: string,
    url: string,
    method: string,
    headers: HeadersInit,
    rid?: number,
) {
    return rollback_on_failure(() => {
        return callHandlerImpl(
            userid,
            path,
            apiVersion,
            url,
            method,
            headers,
            rid,
        );
    });
}

async function callHandlerImpl(
    userid: string | undefined,
    path: string,
    apiVersion: string,
    url: string,
    method: string,
    headers: HeadersInit,
    rid?: number,
) {
    requestContext.method = method;
    requestContext.apiVersion = apiVersion;
    requestContext.path = path;
    requestContext.userId = userid;

    // FIXME: maybe defer creating the transaction until we need one, to avoid doing it for
    // endpoints that don't do any data access. For now, because we always create it above,
    // it should be safe to unwrap.
    await Deno.core.opAsync("op_chisel_create_transaction");

    const init: RequestInit = { method: method, headers: headers };
    if (rid !== undefined) {
        const body = buildReadableStreamForBody(rid);
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
    const res = await handlers[fullPath](req);
    const resHeaders: [string, string][] = [];
    for (const h of res.headers) {
        resHeaders.push(h);
    }
    const reader = res.body?.getReader();
    const read = reader
        ? function () {
            return rollback_on_failure(async () => {
                const v = await reader.read();
                if (v.done) {
                    await Deno.core.opAsync("op_chisel_commit_transaction");
                    return undefined;
                }
                return v.value;
            });
        }
        : undefined;
    return {
        "status": res.status,
        "headers": resHeaders,
        "read": read,
    };
}
