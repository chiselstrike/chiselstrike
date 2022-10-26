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

interface CounterParams<T extends string = string> {
    name: string;
    help?: string;
    labels?: T[];
}

interface IncrementCounterByParams {
    name: string;
    labels: string[];
    value: number;
}

type Labels<T extends string> = Record<T, string>;

export class Counter<T extends string = string> {
    private name: string;
    private label_keys: T[];

    constructor(params: CounterParams<T>) {
        this.name = params.name;
        this.label_keys = params.labels ?? [];
        registerCounter(params);
    }

    with(labels?: Labels<T>): LabeledCounter {
        const label_values = this.label_keys.map((label) =>
            (labels ?? {} as Labels<T>)[label]
        );
        return new LabeledCounter(this.name, label_values);
    }
}

class LabeledCounter {
    private name: string;
    private labels: string[];

    constructor(name: string, labels: string[]) {
        this.name = name;
        this.labels = labels;
    }

    inc(value?: number): void {
        incrementCounterBy({
            name: this.name,
            labels: this.labels,
            value: value ?? 1,
        });
    }
}

function registerCounter({ name, help, labels }: CounterParams) {
    return opSync("op_chisel_register_app_counter", {
        name,
        help: help ?? "",
        labels: labels ?? [],
    });
}

function incrementCounterBy(params: IncrementCounterByParams) {
    return opSync("op_chisel_inc_by_app_counter", params);
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
};
