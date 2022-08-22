// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import type { AuthUser } from "./datastore.ts";

/** Extends the Request class adding ChiselStrike-specific helpers
 *
 * @property {string} version - The current API Version
 * @property {string} endpoint - The current endpoint being called.
 * @property {string} pathParams - This is essentially the URL's path, but with everything before the endpoint name removed.
 * @property {AuthUser} user - The currently logged in user. `undefined` if there isn't one.
 * @property {Query} query - Helper structure containing parsed query string from the URL.
 */
export class ChiselRequest extends Request {
    public query: Query;

    constructor(
        input: string,
        init: RequestInit,
        public version: string,
        public endpoint: string,
        public pathParams: string,
        public user?: AuthUser | undefined,
    ) {
        super(input, init);
        this.query = new Query(new URL(this.url).searchParams);
    }

    /**
     * Returns each component of the arguments part of the path
     *
     * While you could call split() on pathParams directly, this
     * convenience function is useful as it handle empty strings better.
     *
     * For example, for the endpoint `/dev/name` this will return an empty
     * array, while pathParams will be "", and splitting that by "/" returns an
     * array with one element, the empty string
     */
    pathComponents(): string[] {
        return this.pathParams.split("/").filter((n) => n.length != 0);
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
        const v = this.get(paramName);
        if (v !== undefined) {
            const f = Number.parseFloat(v);
            if (Number.isNaN(f)) {
                return undefined;
            } else {
                return f;
            }
        }
        return undefined;
    }

    /**
     * Gets the first query parameter named `paramName` and tries to parse it as boolean. If no such a
     * parameter exists, returns `undefined`
     * If `paramName` key exists, the value is first converted to lowercase and then 'false', '' and
     * '0' are evaluated as false, anything else as true.
     * @param paramName query parameter to be retrieved from the URL's query string.
     */
    getBool(paramName: string): boolean | undefined {
        const v = this.get(paramName);
        if (v !== undefined) {
            switch (v.toLowerCase()) {
                case "false":
                case "0":
                case "":
                    return false;
                default:
                    return true;
            }
        }
        return undefined;
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
}
