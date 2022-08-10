// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { ChiselRequest, requestContext, loggedInUser, responseFromJson } from './chisel.ts';
import { Router, RouterMatch, RouteMap, Middleware, Handler } from './routing.ts';

// HTTP API request that we receive from Rust
type ApiRequest = {
    method: string,
    uri: string,
    headers: [string, string][],
    body: Uint8Array,
    routingPath: string,
    userId: string | undefined,
};

// HTTP API response that we give to Rust
type ApiResponse = {
    status: number,
    headers: [string, string][],
    body: Uint8Array,
};

const versionId: string = Deno.core.opSync('op_chisel_get_version_id');

export async function serve(routeMap: RouteMap): Promise<void> {
    const router = new Router(routeMap);
    Deno.core.opSync('op_chisel_ready');

    while (true) {
        const accepted = await Deno.core.opAsync('op_chisel_accept');
        if (accepted === null) { break; }

        const [apiRequest, responseRid] = accepted;
        const apiResponse = await handleRequest(router, apiRequest);
        Deno.core.opSync('op_chisel_respond', responseRid, apiResponse);
    }
}

async function handleRequest(router: Router, apiRequest: ApiRequest): Promise<ApiResponse> {
    const routerMatch = router.lookup(apiRequest.method, apiRequest.routingPath);
    if (routerMatch === null) {
        return emptyResponse(404);
    }

    // the HTTP request usually specifies only path and query, but we need a full URL; so we resolve the URL
    // from the request with respect to an arbitrary base
    const url = new URL(apiRequest.uri, location.href);

    // initialize the legacy global request context
    // note that this means that we can only handle a single request at a time!
    requestContext.versionId = versionId;
    requestContext.method = apiRequest.method;
    requestContext.headers = apiRequest.headers;
    requestContext.path = url.pathname;
    requestContext.routingPath = apiRequest.routingPath;
    requestContext.userId = apiRequest.userId;

    // we must start the transaction before reading the logged-in user
    await Deno.core.opAsync('op_chisel_begin_transaction');
    const user = await loggedInUser(); // reads `requestContext.userId`

    // convert the internal `apiRequest` to the request that is visible to user code
    const chiselRequest = new ChiselRequest(
        url.toString(),
        {
            method: apiRequest.method,
            headers: apiRequest.headers,
            // Request() complains if there is a body in a GET/HEAD request
            body: (apiRequest.method == 'GET' || apiRequest.method == 'HEAD') 
                ? undefined : apiRequest.body,
        },
        url.pathname,
        versionId,
        user,
        url.searchParams,
        routerMatch.params,
    );

    let response: Response;
    let responseBody: ArrayBuffer;
    try {
        response = await handleRouterMatch(routerMatch, chiselRequest);

        // read the response body before committing the transaction, because user
        // code might still be running while the response is streaming
        responseBody = await response.arrayBuffer();

        await Deno.core.opAsync('op_chisel_commit_transaction');
    } catch (e) {
        let description = '';
        if (e instanceof Error && e.stack !== undefined) {
            description = e.stack;
        } else {
            description = '' + e;
        }
        console.error(`Error in ${apiRequest.method} ${apiRequest.uri}: ${description}`);

        try {
            Deno.core.opSync('op_chisel_rollback_transaction');
        } catch (e) {
            console.error(`Error when rolling back transaction: ${e}`);
        }

        // return an empty response to avoid leaking details about the user error
        // TODO: perhaps we should introduce a "debug mode" that would display a nice error response?
        return emptyResponse(500);
    }

    return {
        status: response.status,
        headers: Array.from(response.headers.entries()),
        body: new Uint8Array(responseBody),
    };
}

function handleRouterMatch(routerMatch: RouterMatch, request: ChiselRequest): Promise<Response> {
    return handleMiddlewareChain(routerMatch.middlewares, routerMatch.handler, request);
}

async function handleMiddlewareChain(
    middlewares: Middleware[],
    handler: Handler,
    request: ChiselRequest,
): Promise<Response> {
    if (middlewares.length == 0) {
        // call the handler function
        const responseLike = await handler.call(undefined, request);
        // TODO: we probably should not convert strings to JSON
        return (responseLike instanceof Response)
            ? responseLike : responseFromJson(responseLike);
    } else {
        // call the middleware handler, passing a callback that will continue in the middleware chain
        const next = (request: ChiselRequest) =>
            handleMiddlewareChain(middlewares.slice(1), handler, request);
        return middlewares[0].handler.call(undefined, request, next);
    }
}

function emptyResponse(status: number): ApiResponse {
    return { status, headers: [], body: new Uint8Array(0) }
}
