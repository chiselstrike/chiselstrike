import { ChiselRequest, responseFromJson } from './chisel.ts';
import { RouteMap } from './routing.ts';

// Corresponds to the `VersionInfo` struct in Rust
type VersionInfo = {
    name: string,
    tag: string,
};

const versionInfo: VersionInfo = Deno.core.opSync('op_chisel_get_version_info');

export function specialBefore(routeMap: RouteMap) {
    async function handleSwagger(req: ChiselRequest): Promise<Response> {
        const paths: Record<string, unknown> = {};
        for (const route of routeMap.routes) {
            paths[route.pathPattern] = {};
        }

        const swagger = {
            swagger: '2.0',
            info: {
                title: versionInfo.name,
                version: versionInfo.tag,
            },
            paths,
        };

        return responseFromJson(swagger);
    }

    routeMap.get('/', handleSwagger);

    // Makes CORS preflights pass.
    routeMap.route('OPTIONS', '.*', (req) => new Response("ok"));
}

export function specialAfter(routeMap: RouteMap) {
}
