// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import type { ChiselRequest } from "./request.ts";

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
    route(method: string | string[], path: string, handler: Handler): this {
        const methods = Array.isArray(method) ? method : [method];
        const pathPattern = path[0] !== "/" ? "/" + path : path;
        this.routes.push({
            methods,
            pathPattern,
            handler,
            middlewares: [],
            legacyFileName: undefined,
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
     * Support for middlewares is experimental and it may change in the future.
     */
    middleware(handler: MiddlewareHandler): this {
        this.middlewares.push({ handler });
        return this;
    }

    // This is called to convert a default export from a file inside `/routes` into a `RouteMap`.
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
        } else if (typeof routes === "object") {
            for (const method in routes) {
                routeMap.route(method, "*", routes[method]);
            }
        } else {
            throw new TypeError(
                `Cannot convert ${typeof routes} into a RouteMap`,
            );
        }
        return routeMap;
    }
}

export type Route = {
    methods: string[];
    pathPattern: string;
    handler: Handler;
    middlewares: Middleware[];
    // TODO: remove this when we no longer need the legacy properties in `ChiselRequest`
    legacyFileName: string | undefined;
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

export type RouteMapLike =
    | RouteMap
    | Record<string, Handler>
    | Handler;

export type Middleware = {
    handler: MiddlewareHandler;
};

export type MiddlewareHandler = (
    request: ChiselRequest,
    next: MiddlewareNext,
) => Promise<Response>;

export type MiddlewareNext = (request: ChiselRequest) => Promise<Response>;

export class Router {
    private routes: RouterRoute[];

    constructor(routeMap: RouteMap) {
        this.routes = routeMap.routes.map((route) =>
            new RouterRoute(route, routeMap)
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
            if (route.testNoMethod(path)) {
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
    noMethodPattern: URLPattern;
    handler: Handler;
    middlewares: Middleware[];
    legacyFileName: string | undefined;

    constructor(route: Route, routeMap: RouteMap) {
        // HACK: we use the hostname part of the URL Pattern to match the method
        const methodPattern = route.methods
            .map((method) => method == "*" ? ".*" : method)
            .join("|");
        this.pattern = new URLPattern(
            route.pathPattern,
            `http://(${methodPattern})`,
        );
        this.noMethodPattern = new URLPattern(
            route.pathPattern,
            "http://dummy-host",
        );
        this.handler = route.handler;
        this.middlewares = route.middlewares.concat(routeMap.middlewares);
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

    testNoMethod(path: string): boolean {
        const baseUrl = "http://dummy-host";
        let matches = this.noMethodPattern.test(path, baseUrl);
        if (!matches && path[path.length - 1] !== "/") {
            matches = this.noMethodPattern.test(path + "/", baseUrl);
        }
        return matches;
    }
}
