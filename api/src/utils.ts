// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

export function opSync(opName: string, a?: unknown, b?: unknown): unknown {
    return Deno.core.opSync(opName, a, b);
}

export function opAsync(
    opName: string,
    a?: unknown,
    b?: unknown,
): Promise<unknown> {
    return Deno.core.opAsync(opName, a, b);
}

/**
 * Acts the same as Object.assign, but performs deep merge instead of a shallow one.
 */
export function mergeDeep(
    target: Record<string, unknown>,
    ...sources: Record<string, unknown>[]
): Record<string, unknown> {
    function isObject(item: unknown): boolean {
        return (item && typeof item === "object" &&
            !Array.isArray(item)) as boolean;
    }

    if (!sources.length) {
        return target;
    }
    const source = sources.shift();

    if (isObject(target) && isObject(source)) {
        for (const key in source) {
            if (isObject(source[key])) {
                if (!target[key]) {
                    Object.assign(target, { [key]: {} });
                }
                mergeDeep(
                    target[key] as Record<string, unknown>,
                    source[key] as Record<string, unknown>,
                );
            } else {
                Object.assign(target, { [key]: source[key] });
            }
        }
    }
    return mergeDeep(target, ...sources);
}

export type JSONValue =
    | string
    | number
    | boolean
    | null
    | { [x: string]: JSONValue }
    | Array<JSONValue>;

/**
 * Gets a secret from the environment
 *
 * To allow a secret to be used, the server has to be run with * --allow-env <YOUR_SECRET>
 *
 * In development mode, all of your environment variables are accessible
 */
export function getSecret(key: string): JSONValue | undefined {
    return opSync("op_chisel_get_secret", key) as JSONValue | undefined;
}

/** Converts a JSON value into a `Response`. */
export function responseFromJson(body: unknown, status = 200) {
    // https://fetch.spec.whatwg.org/#null-body-status
    const isNullBody = status == 101 || status == 103 ||
        status == 204 || status == 205 || status == 304;

    const json = isNullBody ? null : stringifyJson(body);
    return new Response(json, {
        status: status,
        headers: [
            ["content-type", "application/json"],
        ],
    });
}

/** Stringifies a `Json` value into JSON. Handles `Map` and `Set` correctly. */
function stringifyJson(value: unknown, space?: string | number): string {
    function replacer(this: unknown, _key: string, value: unknown): unknown {
        if (value instanceof Map) {
            const obj: Record<string, unknown> = {};
            for (const [k, v] of value.entries()) {
                obj["" + k] = v;
            }
            return obj;
        }

        if (value instanceof Set) {
            return Array.from(value);
        }

        return value;
    }

    return JSON.stringify(value, replacer, space);
}

/** HTTP status codes */
export const HTTP_STATUS = {
    NOT_FOUND: 404,
    METHOD_NOT_ALLOWED: 405,
    INTERNAL_SERVER_ERROR: 500,
};
