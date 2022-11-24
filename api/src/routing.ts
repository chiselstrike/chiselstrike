// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { responseFromJson } from "./utils.ts";
import type { ChiselRequest } from "./request.ts";
import { typeSystem } from "./type_system.ts";

/** Container for HTTP routes and their handlers.
 *
 * This class is used to define HTTP routes in ChiselStrike. Every file under
 * the `routes/` directory in your ChiselStrike project should export a
 * `RouteMap`. For example, to handle HTTP requests for `GET /blog` and `POST
 * /blog/comment`, you might create a file `routes/blog.ts` with this content:
 *
 * ```typescript
 * async function getBlog(req: ChiselRequest) {
 *      // handle `GET /blog`
 * }
 *
 * async function postBlogComment(req: ChiselRequest) {
 *      // handle `POST /blog/comment`
 * }
 *
 * export default new RouteMap()
 *      .get("/", getBlog)
 *      .post("/comment", postBlogComment);
 * ```
 *
 * You can also register another `RouteMap` under a path prefix:
 *
 * ```typescript
 * const commentRoutes = new RouteMap()
 *      .get("/", getAllComments)
 *      .get("/:id", getOneComment)
 *      .post("/", postComment);
 *
 * export default new RouteMap()
 *      .prefix("/comment", commentRoutes)
 *      .prefix("/post", Post.crud()); // the `.crud()` method returns a RouteMap
 * ```
 */
export class RouteMap {
    routes: Route[];
    middlewares: Middleware[];

    /** Creates an empty `RouteMap`. */
    constructor() {
        this.routes = [];
        this.middlewares = [];
        this.addInternalRoutes();
    }

    /** Adds a route to the route map.
     *
     * When an HTTP request matches the given `method` and `path`, we will call
     * the `handler` to handle the request.
     *
     * @param method Either one HTTP method (`"GET"`) or an array of methods
     * (`["GET", "POST"]`) that should be handled by the `handler`. You can also
     * pass `"*"` to handle all HTTP methods.
     *
     * @param path A pattern that must the URL path of the HTTP request. We
     * support patterns from the [URL Pattern API][url-pattern-api], so you can
     * also use "named groups" like `"/post/:id"` that dynamically match a part
     * of the URL. You can read the matched value in the `handler` using
     * `ChiselRequest.params`.
     *
     * @param handler A function that handles the request. It takes a
     * `ChiselRequest` (or `Request`) and returns the response. If the returned
     * value is not a `Response`, we convert it automatically: strings are
     * returned verbatim and other values are converted to JSON. The handler can
     * also be async (it can return a `Promise`).
     *
     * [url-pattern-api]: https://developer.mozilla.org/en-US/docs/Web/API/URL_Pattern_API
     */
    route(
        method: string | string[],
        path: string,
        handler: Handler,
        clientMetadata?: CrudMetadata,
    ): this {
        const methods = Array.isArray(method) ? method : [method];
        const pathPattern = path[0] !== "/" ? "/" + path : path;
        this.routes.push({
            methods,
            pathPattern,
            handler,
            middlewares: [],
            legacyFileName: undefined,
            clientMetadata,
        });
        return this;
    }

    /** Adds routes from another `RouteMap` under a URL prefix.
     *
     * @param path The prefix that is prepended to all routes in `routes`. If
     * you don't want to add any prefix, use `"/"` or `""`. The prefix can also
     * contain patterns (see the documentation for `route()`).
     *
     * @param routeMap A `RouteMap` with routes that will be added to `this`.
     */
    prefix(path: string, routeMap: RouteMap): this {
        // "/foo" -> "/foo"
        // "foo" -> "/foo"
        // "foo/" -> "/foo"
        // "/" -> ""
        if (path[0] !== "/") {
            path = "/" + path;
        }
        if (path[path.length - 1] === "/") {
            path = path.slice(0, path.length - 1);
        }

        for (const route of routeMap.routes) {
            this.routes.push({
                methods: route.methods,
                pathPattern: path + route.pathPattern,
                handler: route.handler,
                middlewares: route.middlewares.concat(routeMap.middlewares),
                legacyFileName: route.legacyFileName,
                clientMetadata: route.clientMetadata,
            });
        }
        return this;
    }

    /** A shorthand for `route()` with `GET` method. */
    get(path: string, handler: Handler): this {
        return this.route("GET", path, handler);
    }

    /** A shorthand for `route()` with `POST` method. */
    post(path: string, handler: Handler): this {
        return this.route("POST", path, handler);
    }

    /** A shorthand for `route()` with `PUT` method. */
    put(path: string, handler: Handler): this {
        return this.route("PUT", path, handler);
    }

    /** A shorthand for `route()` with `DELETE` method. */
    delete(path: string, handler: Handler): this {
        return this.route("DELETE", path, handler);
    }

    /** A shorthand for `route()` with `PATCH` method. */
    patch(path: string, handler: Handler): this {
        return this.route("PATCH", path, handler);
    }

    /** Adds a middleware that will apply to all routes in this route map.
     *
     * The given middleware `handler` will be called before any request handler:
     * it might do some work before or after the request handler, or it may
     * decide not to call the request handler at all. See `MiddlewareHandler`
     * for more details.
     *
     * Support for middlewares is experimental and it may change in the future.
     */
    middleware(handler: MiddlewareHandler): this {
        this.middlewares.push({ handler });
        return this;
    }

    // Convert a default export from a file inside `/routes` into a `RouteMap`.
    // This is an internal, private API.
    // TODO: remove the `legacyFileName` when we no longer need the legacy properties in `ChiselRequest`.
    static convert(routes: RouteMapLike, legacyFileName?: string): RouteMap {
        if (routes instanceof RouteMap) {
            return routes;
        }

        const routeMap = new RouteMap();
        if (typeof routes === "function") {
            const route = {
                methods: ["*"],
                // TODO: replace this with just "/(.*)" when we no longer need the legacy properties in
                // `ChiselRequest`
                pathPattern: "/:legacyPathParams(.*)",
                handler: routes,
                middlewares: [],
                legacyFileName,
            };
            routeMap.routes.push(route);
        } else {
            throw new TypeError(
                `Cannot convert ${typeof routes} into a RouteMap`,
            );
        }
        return routeMap;
    }

    // Adds internal endpoint listing all endpoints.c
    private addInternalRoutes(): this {
        const routeMap = this;
        async function getAllRoutes(_req: ChiselRequest): Promise<Response> {
            const routes = routeMap.routes.map((r) => {
                let clientMetadata = undefined;
                if (r.clientMetadata !== undefined) {
                    clientMetadata = {
                        entityType: typeSystem.findEntity(
                            r.clientMetadata.entityName,
                        ),
                    };
                }
                return {
                    methods: r.methods,
                    pathPattern: r.pathPattern,
                    clientMetadata,
                };
            });
            routes.sort((a, b) => (a.pathPattern > b.pathPattern ? 1 : -1));
            return responseFromJson(routes);
        }
        return this.get("/__chisel_internal/routes", getAllRoutes);
    }
}

export type Route = {
    methods: string[];
    pathPattern: string;
    handler: Handler;
    middlewares: Middleware[];
    // TODO: remove this when we no longer need the legacy properties in `ChiselRequest`
    legacyFileName: string | undefined;
    clientMetadata?: CrudMetadata;
};

/** Metadata used to generate chisel client code.
 */
export type CrudMetadata = {
    entityName: string;
};

/** A request handler that maps HTTP request to an HTTP response. */
export type Handler = (
    req: ChiselRequest,
) => ResponseLike | Promise<ResponseLike>;

/** Anything that we can convert to a `Response`:
 *
 * - `Response` is not modified in any way
 * - `string` is converted using `new Response(string)`
 * - Other values are converted to JSON using `responseFromJson()`
 */
export type ResponseLike = Response | string | unknown;

/** Anything that we can convert to a `RouteMap`:
 *
 * - `RouteMap` is used as-is
 * - `Handler` handles requests for all methods and all paths
 */
export type RouteMapLike =
    | RouteMap
    | Handler;

export type Middleware = {
    handler: MiddlewareHandler;
};

/** A middleware handler that "wraps" the route handlers.
 *
 * When a middleware is registered for a `RouteMap`, we call the middleware
 * handler instead of directly invoking the request handler registered with
 * `RouteMap.route()`.
 *
 * The middleware handler is similar to a normal request handler: it receives a
 * `ChiselRequest` and must produce a `Response`. However, it also receives a
 * `next` callback, which can be used to invoke the original request handler.
 *
 * Support for middlewares is experimental and it may change in the future.
 */
export type MiddlewareHandler = (
    request: ChiselRequest,
    next: MiddlewareNext,
) => Promise<Response>;

export type MiddlewareNext = (request: ChiselRequest) => Promise<Response>;

export class Router {
    private routes: RouterRoute[];

    constructor(routeMap: RouteMap) {
        this.routes = routeMap.routes.map((route) =>
            new RouterRoute(route, routeMap.middlewares)
        );
    }

    lookup(
        method: string,
        path: string,
    ): RouterMatch | "not_found" | "method_not_allowed" {
        for (const route of this.routes) {
            const match = route.match(method, path);
            if (match !== null) {
                return match;
            }
        }

        for (const route of this.routes) {
            if (route.testPathOnly(path)) {
                return "method_not_allowed";
            }
        }

        return "not_found";
    }
}

export type RouterMatch = {
    params: Record<string, string>;
    handler: Handler;
    middlewares: Middleware[];
    legacyFileName: string | undefined;
};

class RouterRoute {
    pattern: URLPattern;
    pathOnlyPattern: URLPattern;
    handler: Handler;
    middlewares: Middleware[];
    legacyFileName: string | undefined;

    constructor(route: Route, routeMapMiddlewares: Middleware[]) {
        // HACK: we use the hostname part of the URL Pattern to match the method
        const methodPattern = route.methods
            .map((method) => method == "*" ? ".*" : method.toLowerCase())
            .join("|");
        this.pattern = new URLPattern(
            `http://(${methodPattern})${route.pathPattern}`,
        );
        this.pathOnlyPattern = new URLPattern(
            `http://dummy-host${route.pathPattern}`,
        );
        this.handler = route.handler;
        this.middlewares = route.middlewares.concat(routeMapMiddlewares);
        this.legacyFileName = route.legacyFileName;
    }

    match(method: string, path: string): RouterMatch | null {
        const methodUrl = `http://${method}`;
        let match = this.pattern.exec(path, methodUrl);
        if (match === null && path[path.length - 1] !== "/") {
            match = this.pattern.exec(path + "/", methodUrl);
        }

        if (match === null) {
            return null;
        }

        return {
            params: match.pathname.groups,
            handler: this.handler,
            middlewares: this.middlewares,
            legacyFileName: this.legacyFileName,
        };
    }

    testPathOnly(path: string): boolean {
        const baseUrl = "http://dummy-host";
        let matches = this.pathOnlyPattern.test(path, baseUrl);
        if (!matches && path[path.length - 1] !== "/") {
            matches = this.pathOnlyPattern.test(path + "/", baseUrl);
        }
        return matches;
    }
}
