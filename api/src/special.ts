import { RouteMap } from './routing.ts';
import { responseFromJson } from './utils.ts';

// Corresponds to the `VersionInfo` struct in Rust
type VersionInfo = {
    name: string,
    tag: string,
};

const versionInfo: VersionInfo = Deno.core.opSync('op_chisel_get_version_info');

export function specialBefore(routeMap: RouteMap) {
    async function handleSwagger(): Promise<Response> {
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
}

export function specialAfter(_routeMap: RouteMap) {
}
