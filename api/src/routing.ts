// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import type { ChiselRequest } from './request.ts';

export class RouteMap {
    routes: Route[]
    middlewares: Middleware[]

    constructor() {
        this.routes = [];
        this.middlewares = [];
    }

    route(method: string | string[], path: string, handler: Handler): this {
        const methods = Array.isArray(method) ? method : [method];
        const pathPattern = path[0] !== '/' ? '/' + path : path;
        this.routes.push({ methods, pathPattern, handler, middlewares: [] });
        return this;
    }

    prefix(path: string, routes: RouteMapLike): this {
        // "/foo" -> "/foo"
        // "foo" -> "/foo"
        // "foo/" -> "/foo"
        // "/" -> ""
        if (path[0] !== '/') {
            path = '/' + path;
        }
        if (path[path.length - 1] === '/') {
            path = path.slice(0, path.length - 1);
        }

        const routeMap = RouteMap.convert(routes);
        for (const route of routeMap.routes) {
            this.routes.push({
                methods: route.methods,
                pathPattern: path + route.pathPattern,
                handler: route.handler,
                middlewares: route.middlewares.concat(routeMap.middlewares),
            });
        }
        return this;
    }

    get(path: string, handler: Handler): this { return this.route('GET', path, handler) }
    post(path: string, handler: Handler): this { return this.route('POST', path, handler) }
    put(path: string, handler: Handler): this { return this.route('PUT', path, handler) }
    delete(path: string, handler: Handler): this { return this.route('DELETE', path, handler) }
    patch(path: string, handler: Handler): this { return this.route('PATCH', path, handler) }

    middleware(handler: MiddlewareHandler): this {
        this.middlewares.push({ handler });
        return this;
    }

    static convert(routes: RouteMapLike): RouteMap {
        if (routes instanceof RouteMap) {
            return routes;
        } else if (typeof routes === 'function') {
            return new RouteMap().route('*', '*', routes)
        } else {
            const routeMap = new RouteMap();
            for (const method in routes) {
                routeMap.route(method, '*', routes[method]);
            }
            return routeMap;
        }
    }
};

export type Route = {
    methods: string[],
    pathPattern: string,
    handler: Handler,
    middlewares: Middleware[],
};
export type Handler = (req: ChiselRequest) => ResponseLike | Promise<ResponseLike>;
export type ResponseLike = Response | unknown;

export type RouteMapLike =
    | RouteMap
    | Record<string, Handler>
    | Handler;

export type Middleware = {
    handler: MiddlewareHandler,
};
export type MiddlewareHandler = (request: ChiselRequest, next: MiddlewareNext) => Promise<Response>;
export type MiddlewareNext = (request: ChiselRequest) => Promise<Response>;


export class Router {
    private routes: RouterRoute[]

    constructor(routeMap: RouteMap) {
        this.routes = routeMap.routes.map((route) => new RouterRoute(route, routeMap));
    }

    lookup(method: string, path: string): RouterMatch | null {
        for (const route of this.routes) {
            const match = route.match(method, path);
            if (match !== null) {
                return match;
            }
        }
        return null;
    }
}

export type RouterMatch = {
    params: Record<string, string>,
    handler: Handler,
    middlewares: Middleware[],
}

class RouterRoute {
    pattern: URLPattern;
    handler: Handler;
    middlewares: Middleware[];

    constructor(route: Route, routeMap: RouteMap) {
        // HACK: we use the hostname part of the URL Pattern to match the method
        const methodPattern = route.methods
            .map(method => method == '*' ? '.*' : method)
            .join('|');
        this.pattern = new URLPattern(route.pathPattern, `http://(${methodPattern})`);
        this.handler = route.handler;
        this.middlewares = route.middlewares.concat(routeMap.middlewares);
    }

    match(method: string, path: string): RouterMatch | null {
        const methodUrl = `http://${method}`;
        let match = this.pattern.exec(path, methodUrl);
        if (match === null && path[path.length - 1] !== '/') {
            match = this.pattern.exec(path + '/', methodUrl);
        }

        if (match === null) {
            return null;
        }

        return { 
            params: match.pathname.groups,
            handler: this.handler,
            middlewares: this.middlewares,
        };
    }
}

