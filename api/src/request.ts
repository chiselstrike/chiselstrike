// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import type { AuthUser } from "./datastore.ts";

/** Extends the Request class adding ChiselStrike-specific helpers
 *
 * @property {string} path - The URL path of this request.
 * @property {string} versionId - The current API Version.
 * @property {AuthUser} user - The currently logged in user. `undefined` if there isn't one.
 * @property {Query} query - Helper class containing parsed query string from the URL.
 * @property {Params} params - Helper class containing parameters from the URL path.
 */
export class ChiselRequest extends Request {
    public readonly path: string;
    public readonly versionId: string;
    public readonly user: AuthUser | undefined;
    public readonly query: Query;
    public readonly params: Params;

    private readonly legacyFileName: string | undefined;

    constructor(
        input: string,
        init: RequestInit,
        path: string,
        versionId: string,
        user: AuthUser | undefined,
        query: URLSearchParams,
        params: Record<string, string>,
        legacyFileName: string | undefined,
    ) {
        super(input, init);
        this.path = path;
        this.versionId = versionId;
        this.user = user;
        this.query = new Query(query);
        this.params = new Params(params);
        this.legacyFileName = legacyFileName;
    }

    /** @deprecated */
    get endpoint(): string {
        return "/" + (this.legacyFileName ?? "");
    }

    /** @deprecated */
    get pathParams(): string {
        return this.params.get("legacyPathParams");
    }

    /** @deprecated */
    pathComponents(): string[] {
        return this.pathParams.split("/").filter((n) => n.length != 0);
    }

    /** @deprecated */
    get version(): string {
        return this.versionId;
    }
}

/** Query is a helper class used to access query parameters parsed from the URL.
 */
export class Query {
    constructor(private params: URLSearchParams) {}

    /**
     * Gets the first query parameter named `paramName`. If no such a parameter exists, returns
     * `undefined`.
     * @param paramName query parameter to be retrieved from the URL's query string.
     */
    get(paramName: string): string | undefined {
        return this.params.get(paramName) ?? undefined;
    }

    /**
     * Gets the first query parameter named `paramName` and tries to parse it as number. If no such a
     * parameter exists or the parsing fails, returns `undefined`.
     * @param paramName query parameter to be retrieved from the URL's query string.
     */
    getNumber(paramName: string): number | undefined {
        return getNumber(this.get(paramName));
    }

    /**
     * Gets the first query parameter named `paramName` and tries to parse it as an integer. If no such a
     * parameter exists or the parsing fails, returns `undefined`.
     * @param paramName query parameter to be retrieved from the URL's query string.
     */
    getInt(paramName: string): number | undefined {
        return getInt(this.get(paramName));
    }

    /**
     * Gets the first query parameter named `paramName` and tries to parse it as boolean. If no such a
     * parameter exists, returns `undefined`
     * If `paramName` key exists, the value is first converted to lowercase and then 'false', '' and
     * '0' are evaluated as false, anything else as true.
     * @param paramName query parameter to be retrieved from the URL's query string.
     */
    getBool(paramName: string): boolean | undefined {
        return getBool(this.get(paramName));
    }

    /**
     * Returns all the values association with a given query parameter.
     */
    getAll(name: string): string[] {
        return this.params.getAll(name);
    }

    /**
     * Returns a Boolean indicating if such a query parameter exists.
     */
    has(name: string): boolean {
        return this.params.has(name);
    }

    /**
     * The entries() method returns an iterator allowing iteration through all
     * key/value pairs contained in the Query. The iterator returns key/value
     * pairs in the same order as they appear in the query string.
     */
    entries(): IterableIterator<[string, string]> {
        return this.params.entries();
    }

    /**
     * The keys() method returns an iterator allowing iteration through all
     * keys contained in the Query. The iterator returns keys
     * in the same order as they appear in the query string.
     */
    keys(): IterableIterator<string> {
        return this.params.keys();
    }

    /**
     * The values() method returns an iterator allowing iteration through all
     * values contained in the Query. The iterator returns values
     * in the same order as they appear in the query string.
     */
    values(): IterableIterator<string> {
        return this.params.values();
    }

    /**
     * The Query object provides an iterator equivalent to the iterator provided
     * by the `entries()` method. For further documentation, please see the docs of
     * `entries()` method.
     */
    [Symbol.iterator](): IterableIterator<[string, string]> {
        return this.entries();
    }

    /**
     * The toString() method returns a query string suitable for use in a URL.
     */
    toString(): string {
        return this.params.toString();
    }
}

/** Params is a helper class used to access route parameters defined in
 * `RouteMap`, extracted from the URL path. */
export class Params {
    constructor(private params: Record<string, string>) {}

    /**
     * Gets the parameter named `paramName`. If the parameter does not exist,
     * throws an error.
     */
    get(paramName: string): string {
        const value = this.params[paramName];
        if (value === undefined) {
            throw new Error(`Undefined parameter '${paramName}'`);
        }
        return value;
    }

    /**
     * Gets the parameter named `paramName` and parses it as a number. If the
     * parameter does not exist, throws an error. If the parsing fails, returns
     * `undefined`.
     */
    getNumber(paramName: string): number | undefined {
        return getNumber(this.get(paramName));
    }

    /**
     * Gets the parameter named `paramName` and parses it as an integer. If the
     * parameter does not exist, throws an error. If the parsing fails, returns
     * `undefined`.
     */
    getInt(paramName: string): number | undefined {
        return getInt(this.get(paramName));
    }

    /**
     * Gets the parameter named `paramName` and parses it as an integer. If the
     * parameter does not exist, throws an error. Parsing a boolean cannot fail,
     * because all values other than `"false"`, `"0"` and `""` are treated as
     * `true`.
     */
    getBool(paramName: string): boolean {
        return getBool(this.get(paramName)) ?? false;
    }
}

function getNumber(value: string | undefined): number | undefined {
    const f = Number.parseFloat(value ?? "");
    return Number.isNaN(f) ? undefined : f;
}

function getInt(value: string | undefined): number | undefined {
    const i = Number.parseInt(value ?? "", 10);
    return Number.isNaN(i) ? undefined : i;
}

function getBool(value: string | undefined): boolean | undefined {
    if (value === undefined) {
        return undefined;
    }
    switch (value.toLowerCase()) {
        case "false":
        case "0":
        case "":
            return false;
        default:
            return true;
    }
}
