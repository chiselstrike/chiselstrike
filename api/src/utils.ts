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

    const jsonBody = valueToJson(body);
    const json = isNullBody ? null : stringifyJson(jsonBody);
    return new Response(json, {
        status: status,
        headers: [
            ["content-type", "application/json"],
        ],
    });
}

const isDebug = opSync("op_chisel_is_debug") as boolean;

/** Stringifies a `Json` value into JSON. */
function stringifyJson(value: unknown, space?: string | number): string {
    if (space === undefined && isDebug) {
        space = 2;
    }
    return JSON.stringify(value, undefined, space);
}

// This function is duplicated in client_lib.ts. If you happen to improve it,
// don't forget to update the other one as well.
function valueToJson(v: unknown): JSONValue {
    if (v === undefined || v === null) {
        return null;
    } else if (typeof v === "string" || v instanceof String) {
        return v as string;
    } else if (typeof v === "number" || v instanceof Number) {
        return v as number;
    } else if (typeof v === "boolean" || v instanceof Boolean) {
        return v as boolean;
    } else if (v instanceof Date) {
        return v.getTime();
    } else if (v instanceof ArrayBuffer || ArrayBuffer.isView(v)) {
        let binary = "";
        const bytes = new Uint8Array(v as ArrayBufferLike);
        const len = bytes.byteLength;
        for (let i = 0; i < len; i++) {
            binary += String.fromCharCode(bytes[i]);
        }
        return btoa(binary);
    } else if (Array.isArray(v)) {
        return v.map(valueToJson);
    } else if (v instanceof Set) {
        const array = [];
        for (const e of v) {
            array.push(valueToJson(e));
        }
        return array;
    } else if (v instanceof Map) {
        const jsonObj: { [x: string]: JSONValue } = {};
        for (const [key, value] of v.entries()) {
            jsonObj["" + key] = valueToJson(value);
        }
        return jsonObj;
    } else if (typeof v === "object") {
        const jsonObj: { [x: string]: JSONValue } = {};
        for (const [key, value] of Object.entries(v)) {
            // Javascript's JSON objects omit undefined values.
            if (value !== undefined) {
                jsonObj[key] = valueToJson(value);
            }
        }
        return jsonObj;
    } else {
        throw new Error(
            `encountered unexpected value type '${typeof v}' when converting to JSON`,
        );
    }
}

/** HTTP status codes */
export const HTTP_STATUS = {
    FORBIDDEN: 403,
    INTERNAL_SERVER_ERROR: 500,
    METHOD_NOT_ALLOWED: 405,
    NOT_FOUND: 404,
};
