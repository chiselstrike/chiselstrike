import { RouteMap } from "./routing.ts";
import { opSync, responseFromJson } from "./utils.ts";

// Corresponds to the `VersionInfo` struct in Rust
type VersionInfo = {
    name: string;
    tag: string;
};

const versionId = opSync("op_chisel_get_version_id") as string;
const versionInfo = opSync("op_chisel_get_version_info") as VersionInfo;

export function specialBefore(routeMap: RouteMap) {
    // Handle Swagger
    routeMap.get("/", () => {
        const paths: Record<string, unknown> = {};
        for (const route of routeMap.routes) {
            paths[`/${versionId}${route.pathPattern}`] = {};
        }

        const swagger = {
            swagger: "2.0",
            info: {
                title: versionInfo.name,
                version: versionInfo.tag,
            },
            paths,
        };
        return responseFromJson(swagger);
    });

    // Handle internal routes reflection. For each route, we return an object listing
    // its accepted methods, path pattern and optional client metadata that are used
    // to generate client code.
    routeMap.get(`/__chiselstrike/routes`, () => {
        const routes = routeMap.routes.map((r) => {
            return {
                methods: r.methods,
                pathPattern: r.pathPattern,
                clientMetadata: r.clientMetadata,
            };
        });
        routes.sort((a, b) => (a.pathPattern > b.pathPattern ? 1 : -1));
        return responseFromJson(routes);
    });
}

export function specialAfter(_routeMap: RouteMap) {
    // there are no special routes to be added after user routes, yet
}
