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

const isDebug = opSync("op_chisel_is_debug") as boolean;

/** Stringifies a `Json` value into JSON. Handles `Map` and `Set` correctly. */
function stringifyJson(value: unknown, space?: string | number): string {
    if (space === undefined && isDebug) {
        space = 2;
    }

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
    FORBIDDEN: 403,
};
