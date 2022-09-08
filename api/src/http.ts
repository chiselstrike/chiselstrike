// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { loggedInUser, requestContext } from "./datastore.ts";
import { ChiselRequest } from "./request.ts";
import { Handler, Middleware, Router, RouterMatch } from "./routing.ts";
import { HTTP_STATUS, opAsync, opSync, responseFromJson } from "./utils.ts";

// HTTP request that we receive from Rust
export type HttpRequest = {
    method: string;
    uri: string;
    headers: [string, string][];
    body: Uint8Array;
    routingPath: string;
    userId: string | undefined;
};

// HTTP response that we give to Rust
export type HttpResponse = {
    status: number;
    headers: [string, string][];
    body: Uint8Array;
};

const versionId = opSync("op_chisel_get_version_id") as string;
const isDebug = opSync("op_chisel_is_debug") as boolean;

// Handle an HTTP request. This should only be called from `run.ts`, see the `run()` function from details.
export async function handleHttpRequest(
    router: Router,
    httpRequest: HttpRequest,
): Promise<HttpResponse> {
    const routerMatch = router.lookup(
        httpRequest.method,
        httpRequest.routingPath,
    );
    if (routerMatch === "not_found") {
        return textResponse(
            HTTP_STATUS.NOT_FOUND,
            `There is no route for ${JSON.stringify(httpRequest.routingPath)}`,
        );
    } else if (routerMatch === "method_not_allowed") {
        return textResponse(
            HTTP_STATUS.METHOD_NOT_ALLOWED,
            `Method ${httpRequest.method} is not supported for ${
                JSON.stringify(httpRequest.routingPath)
            }`,
        );
    }

    // the HTTP request usually specifies only path and query, but we need a full URL; so we resolve the URL
    // from the request with respect to an arbitrary base
    const url = new URL(httpRequest.uri, location.href);

    requestContext.method = httpRequest.method;
    requestContext.userId = httpRequest.userId;

    // we must start the transaction before reading the logged-in user
    await opAsync("op_chisel_begin_transaction");
    const user = await loggedInUser(); // reads `requestContext.userId`

    // convert the internal `httpRequest` to the request that is visible to user code
    const chiselRequest = new ChiselRequest(
        url.toString(),
        {
            method: httpRequest.method,
            headers: httpRequest.headers,
            // Request() complains if there is a body in a GET/HEAD request
            body: (httpRequest.method == "GET" || httpRequest.method == "HEAD")
                ? undefined
                : httpRequest.body,
        },
        url.pathname,
        versionId,
        user,
        url.searchParams,
        routerMatch.params,
        routerMatch.legacyFileName,
    );

    try {
        const response = await handleRouterMatch(routerMatch, chiselRequest);

        // read the response body before committing the transaction, because user
        // code might still be running while the response is streaming
        const responseBody = await response.arrayBuffer();

        await opAsync("op_chisel_commit_transaction");

        return {
            status: response.status,
            headers: Array.from(response.headers.entries()),
            body: new Uint8Array(responseBody),
        };
    } catch (e) {
        let description = "";
        if (e instanceof Error && e.stack !== undefined) {
            description = e.stack;
        } else {
            description = "" + e;
        }
        let message =
            `Error in ${httpRequest.method} ${httpRequest.uri}: ${description}`;

        try {
            opSync("op_chisel_rollback_transaction");
        } catch (e) {
            message += `\nError when rolling back transaction: ${e}`;
        }

        console.error(message);
        if (isDebug) {
            return textResponse(HTTP_STATUS.INTERNAL_SERVER_ERROR, message);
        } else {
            return emptyResponse(HTTP_STATUS.INTERNAL_SERVER_ERROR);
        }
    }
}

function handleRouterMatch(
    routerMatch: RouterMatch,
    request: ChiselRequest,
): Promise<Response> {
    return handleMiddlewareChain(
        routerMatch.middlewares,
        routerMatch.handler,
        request,
        0,
    );
}

async function handleMiddlewareChain(
    middlewares: Middleware[],
    handler: Handler,
    request: ChiselRequest,
    middlewareIndex: number,
): Promise<Response> {
    if (middlewareIndex >= middlewares.length) {
        // call the handler function
        const responseLike = await handler.call(undefined, request);
        if (responseLike instanceof Response) {
            return responseLike;
        } else if (typeof responseLike === "string") {
            return new Response(responseLike);
        } else {
            return responseFromJson(responseLike);
        }
    } else {
        // call the middleware handler, passing a callback that will continue in the middleware chain
        const next = (request: ChiselRequest) =>
            handleMiddlewareChain(
                middlewares,
                handler,
                request,
                middlewareIndex + 1,
            );
        return middlewares[middlewareIndex].handler.call(
            undefined,
            request,
            next,
        );
    }
}

function emptyResponse(status: number): HttpResponse {
    return { status, headers: [], body: new Uint8Array(0) };
}

function textResponse(status: number, text: string): HttpResponse {
    return {
        status,
        headers: [["content-type", "text/plain"]],
        body: new TextEncoder().encode(text),
    };
}
