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
 * Gets a secret from the environment
 *
 * To allow a secret to be used, the server has to be run with * --allow-env <YOUR_SECRET>
 *
 * In development mode, all of your environment variables are accessible
 */
export type JSONValue =
    | string
    | number
    | boolean
    | null
    | { [x: string]: JSONValue }
    | Array<JSONValue>;

export function getSecret(key: string): JSONValue | undefined {
    return opSync("op_chisel_get_secret", key) as JSONValue | undefined;
}

export function responseFromJson(body: unknown, status = 200) {
    // https://fetch.spec.whatwg.org/#null-body-status
    const isNullBody = (status: number): boolean => {
        return status == 101 || status == 204 || status == 205 || status == 304;
    };

    const json = isNullBody(status) ? null : JSON.stringify(body, null, 2);
    return new Response(json, {
        status: status,
        headers: [
            ["content-type", "application/json"],
        ],
    });
}
