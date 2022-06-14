// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

/// <reference lib="deno.core" />
/// <reference lib="dom" />
/// <reference lib="dom.iterable" />

function opSync(opName: string, a?: unknown, b?: unknown): unknown {
    return Deno.core.opSync(opName, a, b);
}

function opAsync(opName: string, a?: unknown, b?: unknown): Promise<unknown> {
    return Deno.core.opAsync(opName, a, b);
}

/**
 * Base class for various Operators applicable on `ChiselCursor`.
 */
abstract class Operator<T> {
    // Read by rust
    readonly type;
    constructor(
        public readonly inner: Operator<T> | undefined,
    ) {
        this.type = this.constructor.name;
    }

    /** Applies specified Operator `op` on each element of passed iterable
     * `iter` creating a new iterable.
     */
    public abstract apply(
        iter: AsyncIterable<T>,
    ): AsyncIterable<T>;

    /** Recursively examines operator chain searching for `ctor` operator.
     * Returns true if found, false otherwise.
     */
    public containsType(ctor: new (...args: never[]) => unknown): boolean {
        if (this instanceof ctor) {
            return true;
        } else if (this.inner === undefined) {
            return false;
        } else {
            return this.inner.containsType(ctor);
        }
    }
}

/**
 * Specifies Entity whose elements are to be fetched.
 */
class BaseEntity<T> extends Operator<T> {
    constructor(
        public name: string,
    ) {
        super(undefined);
    }

    apply(
        _iter: AsyncIterable<T>,
    ): AsyncIterable<T> {
        throw new Error("can't apply BaseEntity operator on an iterable");
    }
}

/**
 * Take operator takes first `count` elements from a collection.
 * The rest is ignored.
 */
class Take<T> extends Operator<T> {
    constructor(
        public readonly count: number,
        inner: Operator<T>,
    ) {
        super(inner);
    }

    apply(
        iter: AsyncIterable<T>,
    ): AsyncIterable<T> {
        const count = this.count;
        return {
            [Symbol.asyncIterator]: async function* () {
                if (count == 0) {
                    return;
                }
                let i = 0;
                for await (const e of iter) {
                    yield e;
                    if (++i >= count) {
                        break;
                    }
                }
            },
        };
    }
}

/**
 * Skip operator skips first `count` elements from a collection.
 */
class Skip<T> extends Operator<T> {
    constructor(
        public readonly count: number,
        inner: Operator<T>,
    ) {
        super(inner);
    }

    apply(
        iter: AsyncIterable<T>,
    ): AsyncIterable<T> {
        const count = this.count;
        return {
            [Symbol.asyncIterator]: async function* () {
                let i = 0;
                for await (const e of iter) {
                    if (++i > count) {
                        yield e;
                    }
                }
            },
        };
    }
}

/**
 * Forces fetch of just the `columns` (fields) of a given entity.
 */
class ColumnsSelect<T, C extends (keyof T)[]>
    extends Operator<Pick<T, C[number]>> {
    constructor(
        public columns: C,
        inner: Operator<T>,
    ) {
        super(inner);
    }

    apply(
        iter: AsyncIterable<T>,
    ): AsyncIterable<Pick<T, C[number]>> {
        const columns = this.columns;
        return {
            [Symbol.asyncIterator]: async function* () {
                for await (const arg of iter) {
                    const newObj: Partial<T> = {};
                    for (const key of columns) {
                        if (arg[key] !== undefined) {
                            newObj[key] = arg[key];
                        }
                    }
                    yield newObj as Pick<T, C[number]>;
                }
            },
        };
    }
}

/**
 * PredicateFilter operator applies `predicate` on each element and keeps
 * only those for which the `predicate` returns true.
 */
class PredicateFilter<T> extends Operator<T> {
    constructor(
        public predicate: (arg: T) => boolean,
        inner: Operator<T>,
    ) {
        super(inner);
    }

    apply(
        iter: AsyncIterable<T>,
    ): AsyncIterable<T> {
        const predicate = this.predicate;
        return {
            [Symbol.asyncIterator]: async function* () {
                for await (const arg of iter) {
                    if (predicate(arg)) {
                        yield arg;
                    }
                }
            },
        };
    }
}

/**
 * ExpressionFilter operator is intended only to be used by Chisel compiler.
 * It applies `predicate` on each element and keeps only those for which
 * the `predicate` returns true. The Chisel compiler provides an `expression`
 * as well which is to be equivalent to the predicate and which is sent to
 * the Rust backend for direct Database evaluation if possible.
 */
class ExpressionFilter<T> extends Operator<T> {
    constructor(
        public predicate: (arg: T) => boolean,
        public expression: Record<string, unknown>,
        inner: Operator<T>,
    ) {
        super(inner);
    }

    apply(
        iter: AsyncIterable<T>,
    ): AsyncIterable<T> {
        const predicate = this.predicate;
        return {
            [Symbol.asyncIterator]: async function* () {
                for await (const arg of iter) {
                    if (predicate(arg)) {
                        yield arg;
                    }
                }
            },
        };
    }
}

/**
 * SortKey specifies that sorting over a `fieldname` is to be done in
 * `ascending` (if true) or descending manner.
 */
class SortKey<T> {
    constructor(
        public fieldName: keyof T,
        public ascending = true,
    ) {}
}

/**
 * SortBy operator sorts elements by sorting `keys`in lexicographicall manner.
 */
class SortBy<T> extends Operator<T> {
    constructor(
        private keys: SortKey<T>[],
        inner: Operator<T>,
    ) {
        super(inner);
    }

    apply(
        iter: AsyncIterable<T>,
    ): AsyncIterable<T> {
        const keys = this.keys;
        return {
            [Symbol.asyncIterator]: async function* () {
                const elements = [];
                for await (const e of iter) {
                    elements.push(e);
                }
                elements.sort(
                    (lhs: T, rhs: T) => {
                        for (const key of keys) {
                            let [l, r] = [
                                lhs[key.fieldName],
                                rhs[key.fieldName],
                            ];
                            if (key.ascending) {
                                [l, r] = [r, l];
                            }
                            if (l != r) {
                                return l < r ? 1 : -1;
                            }
                        }
                        return 0;
                    },
                );
                for (const e of elements) {
                    yield e;
                }
            },
        };
    }
}

/** ChiselCursor is a lazy iterator that will be used by ChiselStrike to construct an optimized query. */
export class ChiselCursor<T> {
    constructor(
        private baseConstructor: { new (): T },
        private inner: Operator<T>,
    ) {}
    /** Force ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    select<C extends (keyof T)[]>(
        ...columns: C
    ): ChiselCursor<Pick<T, C[number]>> {
        return new ChiselCursor(
            this.baseConstructor,
            new ColumnsSelect(columns, this.inner),
        );
    }

    /** Restricts this cursor to contain only at most `count` elements */
    take(count: number): ChiselCursor<T> {
        return new ChiselCursor(
            this.baseConstructor,
            new Take(count, this.inner),
        );
    }

    /** Skips the first `count` elements of this cursor. */
    skip(count: number): ChiselCursor<T> {
        return new ChiselCursor(
            this.baseConstructor,
            new Skip(count, this.inner),
        );
    }

    /**
     * Restricts this cursor to contain only elements that match the given @predicate.
     */
    filter(
        predicate: (arg: T) => boolean,
    ): ChiselCursor<T>;
    /**
     * Restricts this cursor to contain just the objects that match the `Partial`
     * object `restrictions`.
     */
    filter(restrictions: Partial<T>): ChiselCursor<T>;

    // Common implementation for filter overloads.
    filter(arg1: ((arg: T) => boolean) | Partial<T>): ChiselCursor<T> {
        if (typeof arg1 == "function") {
            return new ChiselCursor(
                this.baseConstructor,
                new PredicateFilter(
                    arg1,
                    this.inner,
                ),
            );
        } else {
            const restrictions = arg1;
            const expr = restrictionsToFilterExpr(restrictions);
            if (expr === undefined) {
                // If it's an empty restriction, no need to create an empty filter.
                return this;
            }
            const predicate = (arg: T) => {
                for (const key in restrictions) {
                    if (restrictions[key] === undefined) {
                        continue;
                    }
                    if (arg[key] != restrictions[key]) {
                        return false;
                    }
                }
                return true;
            };
            return new ChiselCursor(
                this.baseConstructor,
                new ExpressionFilter(
                    predicate,
                    expr,
                    this.inner,
                ),
            );
        }
    }

    // Filtering function used by Chisel Compiler. Not intended for direct usage.
    __filter(
        exprPredicate: (arg: T) => boolean,
        expression: Record<string, unknown>,
        postPredicate?: (arg: T) => boolean,
    ) {
        let op: Operator<T> = new ExpressionFilter(
            exprPredicate,
            expression,
            this.inner,
        );
        if (postPredicate !== undefined) {
            op = new PredicateFilter(postPredicate, op);
        }
        return new ChiselCursor(this.baseConstructor, op);
    }

    /**
     * Sorts cursor elements.
     *
     * @param key specifies which attribute of `T` is to be used as a sort key.
     * @param ascending if true, the sort will be ascending. Descending otherwise.
     *
     * Note: the sort is not guaranteed to be stable.
     */
    sortBy(key: keyof T, ascending = true): ChiselCursor<T> {
        return new ChiselCursor(
            this.baseConstructor,
            new SortBy(
                [new SortKey<T>(key, ascending)],
                this.inner,
            ),
        );
    }

    /**
     * Finds minimal value over all elements using their `key` attribute.
     *
     * @param key specifies which attribute of `T` is to be used to chose the minimum.
     * @returns minimal value of attribute called `key` across all elements. Undefined
     * values are ignored. If there are no elements or non-undefined values,
     * the function returns undefined.
     */
    async minBy<K extends keyof T>(key: K): Promise<T[K] | undefined> {
        let min = undefined;
        for await (const e of this) {
            const val = e[key];
            if (min === undefined) {
                min = val;
            } else if (val !== undefined && val < min) {
                min = val;
            }
        }
        return min;
    }

    /**
     * Finds maximal value over all elements using their `key` attribute.
     *
     * @param key specifies which attribute of `T` is to be used to chose the maximum.
     * @returns maximal value of attribute called `key` across all elements. Undefined
     * values are ignored. If there are no elements or non-undefined values,
     * the function returns undefined.
     */
    async maxBy<K extends keyof T>(key: K): Promise<T[K] | undefined> {
        let max = undefined;
        for await (const e of this) {
            const val = e[key];
            if (max === undefined) {
                max = val;
            } else if (val !== undefined && val > max) {
                max = val;
            }
        }
        return max;
    }

    /** Executes the function `func` for each element of this cursor. */
    async forEach(func: (arg: T) => void): Promise<void> {
        for await (const t of this) {
            func(t);
        }
    }

    /** Converts this cursor to an Array.
     *
     * Use this with caution as the result set can be very big.
     * It is recommended that you take() first to cap the maximum number of elements. */
    async toArray(): Promise<T[]> {
        const arr = [];
        for await (const t of this) {
            arr.push(t);
        }
        return arr;
    }

    /** ChiselCursor implements asyncIterator, meaning you can use it in any asynchronous context. */
    [Symbol.asyncIterator](): AsyncIterator<T> {
        let iter = this.evalOpsRecursive(this.inner);
        if (iter === undefined) {
            iter = this.runChiselQuery(this.inner);
        }
        return iter[Symbol.asyncIterator]();
    }

    /** Performs recursive descent via Operator.inner examining the whole operator
     * chain. If PredicateFilter is encountered, a backend query is generated and
     * all subsequent operations are applied on the resulting async iterable in
     * TypeScript. In such a case, the function returns the resulting AsyncIterable.
     * If no PredicateFilter is found, undefined is returned.
     */
    private evalOpsRecursive(
        op: Operator<T>,
    ): AsyncIterable<T> | undefined {
        if (op.inner === undefined) {
            return undefined;
        }
        let iter = this.evalOpsRecursive(op.inner);
        if (iter === undefined && op instanceof PredicateFilter) {
            iter = this.runChiselQuery(op.inner);
        }
        if (iter !== undefined) {
            return op.apply(iter);
        } else {
            return undefined;
        }
    }

    private runChiselQuery(
        op: Operator<T>,
    ): AsyncIterable<T> {
        const ctor = op.containsType(ColumnsSelect)
            ? undefined
            : this.baseConstructor;
        return {
            [Symbol.asyncIterator]: async function* () {
                const rid = opSync(
                    "op_chisel_relational_query_create",
                    op,
                    requestContext,
                ) as number;
                try {
                    while (true) {
                        const properties = await opAsync(
                            "op_chisel_query_next",
                            rid,
                        ) as T | null; // FIXME: This is wrong, we can get less than T with ColumnsSelect.

                        if (properties === null) {
                            break;
                        }
                        if (ctor !== undefined) {
                            const result = new ctor();
                            Object.assign(result, properties);
                            yield result;
                        } else {
                            yield properties;
                        }
                    }
                } finally {
                    Deno.core.tryClose(rid);
                }
            },
        };
    }
}

/** Extends the Request class adding ChiselStrike-specific helpers
 *
 * @property {string} version - The current API Version
 * @property {string} endpoint - The current endpoint being called.
 * @property {string} pathParams - This is essentially the URL's path, but with everything before the endpoint name removed.
 * @property {AuthUser} user - The currently logged in user. `undefined` if there isn't one.
 */
export class ChiselRequest extends Request {
    constructor(
        input: string,
        init: RequestInit,
        public version: string,
        public endpoint: string,
        public pathParams: string,
        public user?: AuthUser | undefined,
    ) {
        super(input, init);
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

export function chiselIterator<T extends ChiselEntity>(
    type: { new (): T },
) {
    const b = new BaseEntity<T>(type.name);
    return new ChiselCursor<T>(type, b);
}

/** ChiselEntity is a class that ChiselStrike user-defined entities are expected to extend.
 *
 * It provides properties that are inherent to a ChiselStrike entity, like an id, and static
 * methods that can be used to obtain a `ChiselCursor`.
 */
export class ChiselEntity {
    /** UUID identifying this object. */
    id?: string;

    /**
     * Builds a new entity.
     *
     * @param properties The properties of the created entity. If more than one property
     * is passed, the expected order of assignment is the same as Object.assign.
     *
     * @example
     * ```typescript
     * export class User extends ChiselEntity {
     *   username: string,
     *   email: string,
     * }
     * // Create an entity from object literal:
     * const user = User.build({ username: "alice", email: "alice@example.com" });
     * // Create an entity from JSON:
     * const userJson = JSON.parse('{"username": "alice", "email": "alice@example.com"}');
     * const anotherUser = User.build(userJson);
     *
     * // Create an entity from different JSON objects:
     * const otherUserJson = JSON.parse('{"username": "alice"}, {"email": "alice@example.com"}');
     * const yetAnotherUser = User.build(userJson);
     *
     * // now optionally save them to the backend
     * await user.save();
     * await anotherUser.save();
     * await yetAnotherUser.save();
     * ```
     * @returns The persisted entity with given properties and the `id` property set.
     */
    static build<T extends ChiselEntity>(
        this: { new (): T },
        ...properties: Partial<T>[]
    ): T {
        const result = new this();
        Object.assign(result, ...properties);
        return result;
    }

    /** saves the current object into the backend */
    async save() {
        ensureNotGet();
        type IdsJson = { id: string; children: Record<string, IdsJson> };
        const jsonIds = await opAsync("op_chisel_store", {
            name: this.constructor.name,
            value: this,
        }, requestContext) as IdsJson;
        function backfillIds(this_: ChiselEntity, jsonIds: IdsJson) {
            this_.id = jsonIds.id;
            for (const [fieldName, value] of Object.entries(jsonIds.children)) {
                const child = (this_ as unknown as Record<string, unknown>)[
                    fieldName
                ];
                backfillIds(child as ChiselEntity, value);
            }
        }
        backfillIds(this, jsonIds);
    }

    /** Returns a `ChiselCursor` containing all elements of type T known to ChiselStrike.
     *
     * Note that `ChiselCursor` is a lazy iterator, so this doesn't mean a query will be generating fetching all elements at this point. */
    static cursor<T extends ChiselEntity>(
        this: { new (): T },
    ): ChiselCursor<T> {
        return chiselIterator<T>(this);
    }

    /**
     * Return all entities of type T.
     */
    static async findAll<T extends ChiselEntity>(
        this: { new (): T },
        take?: number,
    ): Promise<T[]> {
        let it = chiselIterator<T>(this);
        if (take) {
            it = it.take(take);
        }
        return await it.toArray();
    }

    /**
     * Returns all entities of type T for which the given `predicate` returns true.
     * You can optionaly specify `take` parameter that will limit the number of
     * results to at most `take` elements.
     */
    static async findMany<T extends ChiselEntity>(
        this: { new (): T },
        predicate: (arg: T) => boolean,
        take?: number,
    ): Promise<T[]>;

    /**
     * Returns all entities of type T matching given `restrictions`.
     * You can optionaly specify `take` parameter that will limit the number of
     * results to at most `take` elements.
     */
    static async findMany<T extends ChiselEntity>(
        this: { new (): T },
        restrictions: Partial<T>,
        take?: number,
    ): Promise<T[]>;

    static async findMany<T extends ChiselEntity>(
        this: { new (): T },
        arg1: ((arg: T) => boolean) | Partial<T>,
        take?: number,
    ): Promise<T[]> {
        let it = undefined;
        if (typeof arg1 == "function") {
            it = chiselIterator<T>(this).filter(arg1);
        } else {
            it = chiselIterator<T>(this).filter(arg1);
        }
        if (take !== undefined) {
            it = it.take(take);
        }
        return await it.toArray();
    }

    // FindMany function used by Chisel Compiler. Not intended for direct usage.
    static async __findMany<T extends ChiselEntity>(
        this: { new (): T },
        exprPredicate: (arg: T) => boolean,
        expression: Record<string, unknown>,
        postPredicate?: (arg: T) => boolean,
        take?: number,
    ): Promise<T[]> {
        let it = chiselIterator<T>(this).__filter(
            exprPredicate,
            expression,
            postPredicate,
        );
        if (take !== undefined) {
            it = it.take(take);
        }
        return await it.toArray();
    }

    /**
     * Returns a single object of type T for which the given `predicate` returns true.
     *
     * If more than one match is found, any is returned
     */
    static async findOne<T extends ChiselEntity>(
        this: { new (): T },
        predicate: (arg: T) => boolean,
    ): Promise<T | undefined>;

    /** Returns a single object that matches the `Partial` object `restrictions` passed as its parameter.
     *
     * If more than one match is found, any is returned. */
    static async findOne<T extends ChiselEntity>(
        this: { new (): T },
        restrictions: Partial<T>,
    ): Promise<T | undefined>;

    static async findOne<T extends ChiselEntity>(
        this: { new (): T },
        arg1: ((arg: T) => boolean) | Partial<T>,
    ): Promise<T | undefined> {
        let it = undefined;
        if (typeof arg1 == "function") {
            it = chiselIterator<T>(this).filter(arg1);
        } else {
            it = chiselIterator<T>(this).filter(arg1);
        }
        for await (const value of it) {
            return value;
        }
        return undefined;
    }

    // findOne function used by Chisel Compiler. Not intended for direct usage.
    static async __findOne<T extends ChiselEntity>(
        this: { new (): T },
        exprPredicate: (arg: T) => boolean,
        expression: Record<string, unknown>,
        postPredicate?: (arg: T) => boolean,
    ): Promise<T | undefined> {
        const it = chiselIterator<T>(this).__filter(
            exprPredicate,
            expression,
            postPredicate,
        );
        for await (const value of it) {
            return value;
        }
        return undefined;
    }

    /**
     * Deletes all entities that match the `restrictions` object.
     *
     * @example
     * ```typescript
     * export class User extends ChiselEntity {
     *   username: string,
     *   email: string,
     * }
     * const user = User.build({ username: "alice", email: "alice@example.com" });
     * await user.save();
     *
     * await User.delete({ email: "alice@example.com"})
     * ```
     */
    static async delete<T extends ChiselEntity>(
        this: { new (): T },
        restrictions: Partial<T>,
    ): Promise<void> {
        ensureNotGet();
        await opAsync("op_chisel_entity_delete", {
            typeName: this.name,
            filterExpr: restrictionsToFilterExpr(restrictions),
        }, requestContext);
    }

    /**
     * Generates endpoint code to handle REST methods GET/PUT/POST/DELETE for this entity.
     *
     * @example
     *
     * Put this in the file 'endpoints/comments.ts':
     * ```typescript
     * import { Comment } from "../models/comment";
     * export default Comment.crud();
     * ```
     *
     * This results in a /comments endpoint that correctly handles all REST methods over Comment. The default  endpoints are:
     *
     * * **POST:**
     *     * `/comments`             Creates a new object. Payload is a JSON with the properties of `Comment` as keys.
     *
     * * **GET:**
     *     * `/comments`                  returns an array with all elements (use carefully in datasets expected to be large)
     *     * `/comments?<filters>`        returns all elements that match filters. For more details, please refer to our documentation.
     *     * `/comments/:id`              returns the element with the given ID.
     *
     * * **DELETE:**
     *     * `/comments/:id`              deletes the element with the given ID.
     *     * `/comments?<filters>`        deletes all elements that match filters. For more details, please refer to our documentation.
     *     * `/comments?all=true`         deletes all elements
     *
     * * **PUT:**
     *     * `/comments/:id`         overwrites the element with the given ID. Payload is a JSON with the properties of `Comment` as keys
     *
     * If you need more control over which method to generate and their behavior, see the top-level `crud()` function
     *
     * @returns A request-handling function suitable as a default export in an endpoint.
     */
    static crud(_ignored?: string) {
        return crud(this, "");
    }

    /**
     * Creates a new object and persists it, in a single step
     *
     * @example
     * ```typescript
     * export class User extends ChiselEntity {
     *   username: string,
     *   email: string,
     * }
     * const user = await User.create({ username: "alice", email: "alice@example.com" });
     * ```
     *
     * Equivalent to calling `const user = User.build(...); await user.save()`
     */
    static async create<T extends ChiselEntity>(
        this: { new (): T },
        ...properties: Record<string, unknown>[]
    ): Promise<T> {
        const result = new this();
        Object.assign(result, ...properties);
        await result.save();
        return result;
    }
}

function restrictionsToFilterExpr<T extends ChiselEntity>(
    restrictions: Partial<T>,
): Record<string, unknown> | undefined {
    let expr = undefined;
    for (const key in restrictions) {
        if (restrictions[key] === undefined) {
            continue;
        }
        const cmpExpr = {
            exprType: "Binary",
            left: {
                exprType: "Property",
                object: { exprType: "Parameter", position: 0 },
                property: key,
            },
            op: "Eq",
            right: {
                exprType: "Literal",
                value: restrictions[key],
            },
        };
        if (expr === undefined) {
            expr = cmpExpr;
        } else {
            expr = {
                exprType: "Binary",
                left: cmpExpr,
                op: "And",
                right: expr,
            };
        }
    }
    return expr;
}

export class AuthUser extends ChiselEntity {
    emailVerified?: string;
    name?: string;
    email?: string;
    image?: string;
}

/**
 * Gets a secret from the environment
 *
 * To allow a secret to be used, the server has to be run with * --allow-env <YOUR_SECRET>
 *
 * In development mode, all of your environment variables are accessible
 */
type JSONValue =
    | string
    | number
    | boolean
    | null
    | { [x: string]: JSONValue }
    | Array<JSONValue>;

export function getSecret(key: string): JSONValue | undefined {
    const { Some } = opSync("op_chisel_get_secret", key) as {
        Some?: JSONValue;
    };
    return Some;
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

export function labels(..._val: string[]) {
    return <T>(_target: T, _propertyName: string) => {
        // chisel-decorator, no content
    };
}

export function unique(_target: unknown, _name: string): void {
    // chisel-decorator, no content
}

/** Returns the currently logged-in user or null if no one is logged in. */
export async function loggedInUser(): Promise<AuthUser | undefined> {
    const id = requestContext.userId;
    if (id === undefined) {
        return undefined;
    }
    return await AuthUser.findOne({ id });
}

function ensureNotGet() {
    if (requestContext.method === "GET") {
        throw new Error("Mutating the backend is not allowed during GET");
    }
}

export const requestContext: {
    path: string;
    method: string;
    headers: Record<string, string>;
    apiVersion: string;
    userId?: string;
} = {
    path: "",
    method: "",
    headers: {},
    apiVersion: "",
};

// TODO: BEGIN: this should be in another file: crud.ts

// TODO: BEGIN: when module import is fixed:
//     import { parse as regExParamParse } from "regexparam";
// or:
//     import { parse as regExParamParse } from "regexparam";
// In the meantime, the regExParamParse function is copied from
// https://deno.land/x/regexparam@v2.0.0/src/index.js under MIT License included
// below. ChiselStrike added the TS signature and minor cleanups.
//
// Copyright (c) Luke Edwards <luke.edwards05@gmail.com> (lukeed.com)
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
export function regExParamParse(str: string, loose: boolean) {
    let tmp, pattern = "";
    const keys = [], arr = str.split("/");
    arr[0] || arr.shift();

    while ((tmp = arr.shift())) {
        const c = tmp[0];
        if (c === "*") {
            keys.push("wild");
            pattern += "/(.*)";
        } else if (c === ":") {
            const o = tmp.indexOf("?", 1);
            const ext = tmp.indexOf(".", 1);
            keys.push(tmp.substring(1, ~o ? o : ~ext ? ext : tmp.length));
            pattern += !!~o && !~ext ? "(?:/([^/]+?))?" : "/([^/]+?)";
            if (~ext) pattern += (~o ? "?" : "") + "\\" + tmp.substring(ext);
        } else {
            pattern += "/" + tmp;
        }
    }

    return {
        keys: keys,
        pattern: new RegExp("^" + pattern + (loose ? "(?=$|/)" : "/?$"), "i"),
    };
}
// TODO: END: when module import is fixed

type ChiselEntityClass<T extends ChiselEntity> = {
    new (): T;
    findOne: (_: { id: string }) => Promise<T | undefined>;
    findMany: (_: Partial<T>) => Promise<T[]>;
    build: (...properties: Record<string, unknown>[]) => T;
    delete: (restrictions: Partial<T>) => Promise<void>;
    cursor: () => ChiselCursor<T>;
};

type GenericChiselEntityClass = ChiselEntityClass<ChiselEntity>;

/**
 * Creates a path parser from a template using regexparam.
 *
 * @param pathTemplate the path template such as `/static`, `/param/:id/:otherParam`...
 * @param loose if true, it can match longer paths. False by default
 * @returns function that can parse paths given as string.
 * @see https://deno.land/x/regexparam@v2.0.0
 */
export function createPathParser<T extends Record<string, unknown>>(
    pathTemplate: string,
    loose = false,
): ((path: string) => T) {
    const { pattern, keys: keysOrFalse } = regExParamParse(pathTemplate, loose);
    if (typeof keysOrFalse === "boolean") {
        throw new Error(
            `invalid pathTemplate=${pathTemplate}, expected string`,
        );
    }
    const keys = keysOrFalse;
    return function pathParser(path: string): T {
        const matches = pattern.exec(path);
        return keys.reduce(
            (acc: Record<string, unknown>, key: string, index: number) => {
                acc[key] = matches?.[index + 1];
                return acc;
            },
            {},
        ) as T;
    };
}

/**
 * Creates a path parser from a template using regexparam.
 *
 * @param pathTemplate the path template such as `/static`, `/param/:id/:otherParam`...
 * @param loose if true, it can match longer paths. False by default
 * @returns function that can parse paths given in URL.pathname.
 * @see https://deno.land/x/regexparam@v2.0.0
 */
export function createURLPathParser<T extends Record<string, unknown>>(
    pathTemplate: string,
    loose = false,
): ((url: URL) => T) {
    const pathParser = createPathParser<T>(pathTemplate, loose);
    return (url: URL): T => pathParser(url.pathname);
}

/** Creates a Response object from response body and status. */
export type CRUDCreateResponse = (
    body: unknown,
    status: number,
) => (Promise<Response> | Response);

export type CRUDBaseParams = {
    /** identifier of the object being manipulated, if any */
    id?: string;
    /** ChiselStrike's version/branch the server is running,
     * such as 'dev' for endpoint '/dev/example'
     * when using 'chisel apply --version dev'
     */
    chiselVersion: string;
};

export type CRUDMethodSignature<
    T extends ChiselEntity,
    E extends ChiselEntityClass<T>,
    P extends CRUDBaseParams = CRUDBaseParams,
> = (
    entity: E,
    req: Request,
    params: P,
    url: URL,
    createResponse: CRUDCreateResponse,
) => Promise<Response>;

/**
 * A dictionary mapping HTTP verbs into corresponding REST methods that process a Request and return a Response.
 */
export type CRUDMethods<
    T extends ChiselEntity,
    E extends ChiselEntityClass<T>,
    P extends CRUDBaseParams = CRUDBaseParams,
> = {
    GET: CRUDMethodSignature<T, E, P>;
    POST: CRUDMethodSignature<T, E, P>;
    PUT: CRUDMethodSignature<T, E, P>;
    DELETE: CRUDMethodSignature<T, E, P>;
};

export type CRUDCreateResponses<
    T extends ChiselEntity,
    E extends ChiselEntityClass<T>,
    P extends CRUDBaseParams = CRUDBaseParams,
> = {
    [K in keyof CRUDMethods<T, E, P>]: CRUDCreateResponse;
};

/**
 * Fetches crud data based on crud `url`.
 */
async function fetchEntitiesCrud<T extends ChiselEntity>(
    type: { new (): T },
    url: string,
): Promise<T[]> {
    const results = await opAsync(
        "op_chisel_crud_query",
        {
            typeName: type.name,
            url,
        },
        requestContext,
    );
    return results as T[];
}

async function deleteEntitiesCrud<T extends ChiselEntity>(
    type: { new (): T },
    url: string,
): Promise<void> {
    await opAsync(
        "op_chisel_crud_delete",
        {
            typeName: type.name,
            url,
        },
        requestContext,
    );
}

const defaultCrudMethods: CRUDMethods<ChiselEntity, GenericChiselEntityClass> =
    {
        // Returns a specific entity matching params.id (if present) or all entities matching the filter in the `filter` URL parameter.
        GET: async (
            entity: GenericChiselEntityClass,
            _req: Request,
            params: CRUDBaseParams,
            url: URL,
            createResponse: CRUDCreateResponse,
        ) => {
            const { id } = params;
            if (id) {
                const u = await entity.findOne({ id });
                return createResponse(u ?? "Not found", u ? 200 : 404);
            } else {
                return createResponse(
                    await fetchEntitiesCrud(entity, url.href),
                    200,
                );
            }
        },
        // Creates and returns a new entity from the `req` payload. Ignores the payload's id property and assigns a fresh one.
        POST: async (
            entity: GenericChiselEntityClass,
            req: Request,
            _params: CRUDBaseParams,
            _url: URL,
            createResponse: CRUDCreateResponse,
        ) => {
            const u = entity.build(await req.json());
            u.id = undefined;
            await u.save();
            return createResponse(u, 200);
        },
        // Updates and returns the entity matching params.id (which must be set) from the `req` payload.
        PUT: async (
            entity: GenericChiselEntityClass,
            req: Request,
            params: CRUDBaseParams,
            _url: URL,
            createResponse: CRUDCreateResponse,
        ) => {
            const { id } = params;
            if (!id) {
                return createResponse(
                    "PUT requires item ID in the URL",
                    400,
                );
            }
            const u = entity.build(await req.json());
            u.id = id;
            await u.save();
            return createResponse(u, 200);
        },
        // Deletes the entity matching params.id (if present) or all entities matching the filter in the `filter` URL parameter. One of the two must be present.
        DELETE: async (
            entity: GenericChiselEntityClass,
            _req: Request,
            params: CRUDBaseParams,
            url: URL,
            createResponse: CRUDCreateResponse,
        ) => {
            const { id } = params;
            if (id) {
                await entity.delete({ id });
                return createResponse(`Deleted ID ${id}`, 200);
            } else {
                await deleteEntitiesCrud(entity, url.href);
                return createResponse(
                    `Deleted entities matching ${url.search}`,
                    200,
                );
            }
        },
    } as const;

/**
 * These methods can be used as `customMethods` in `ChiselStrike.crud()`.
 *
 * @example
 * Put this in the file 'endpoints/comments.ts':
 * ```typescript
 * import { Comment } from "../models/comment";
 * export default crud(
 *   Comment,
 *   ':id',
 *   {
 *     PUT: standardCRUDMethods.notFound, // do not update, instead returns 404
 *     DELETE: standardCRUDMethods.methodNotAllowed, // do not delete, instead returns 405
 *   },
 * );
 * ```
 */
export const standardCRUDMethods = {
    forbidden: (
        _entity: GenericChiselEntityClass,
        _req: Request,
        _params: CRUDBaseParams,
        _url: URL,
        createResponse: CRUDCreateResponse,
    ) => Promise.resolve(createResponse("Forbidden", 403)),
    notFound: (
        _entity: GenericChiselEntityClass,
        _req: Request,
        _params: CRUDBaseParams,
        _url: URL,
        createResponse: CRUDCreateResponse,
    ) => Promise.resolve(createResponse("Not Found", 404)),
    methodNotAllowed: (
        _entity: GenericChiselEntityClass,
        _req: Request,
        _params: CRUDBaseParams,
        _url: URL,
        createResponse: CRUDCreateResponse,
    ) => Promise.resolve(createResponse("Method Not Allowed", 405)),
} as const;

/**
 * Generates endpoint code to handle REST methods GET/PUT/POST/DELETE for this entity.
 * @example
 * Put this in the file 'endpoints/comments.ts':
 * ```typescript
 * import { Comment } from "../models/comment";
 * export default crud(Comment, ":id");
 * ```
 * This results in a /comments endpoint that correctly handles all REST methods over Comment.
 * @param entity Entity type
 * @param urlTemplateSuffix A suffix to be added to the Request URL (see https://deno.land/x/regexparam for syntax).
 *   Some CRUD methods rely on parts of the URL to identify the resource to apply to. Eg, GET /comments/1234
 *   returns the comment entity with id=1234, while GET /comments returns all comments. This parameter describes
 *   how to find the relevant parts in the URL. Default CRUD methods (see `defaultCrudMethods`) look for the :id
 *   part in this template to identify specific entity instances. If there is no :id in the template, then ':id'
 *   is automatically added to its end. Custom methods can use other named parts.
 * @param config Configure the CRUD behavior:
 *  - `customMethods`: custom request handlers overriding the defaults.
 *     Each present property overrides that method's handler. You can use `standardCRUDMethods` members here to
 *     conveniently reject some actions. When `customMethods` is absent, we use methods from `defaultCrudMethods`.
 *     Note that these default methods look for the `id` property in their `params` argument; if set, its value is
 *     the id of the entity to process. Conveniently, the default `urlTemplate` parser sets this property from the
 *     `:id` pattern.
 *  - `createResponses`: if present, a dictionary of method-specific Response creators.
 *  - `defaultCreateResponse`: default function to create all responses if `createResponses` entry is not provided.
 *     Defaults to `responseFromJson()`.
 *  - `parsePath`: parses the URL path instead of https://deno.land/x/regexparam. The parsing result is passed to
 *     CRUD methods as the `params` argument.
 * @returns A request-handling function suitable as a default export in an endpoint.
 */
export function crud<
    T extends ChiselEntity,
    E extends ChiselEntityClass<T>,
    P extends CRUDBaseParams = CRUDBaseParams,
>(
    entity: E,
    urlTemplateSuffix: string,
    config?: {
        customMethods?: Partial<CRUDMethods<T, ChiselEntityClass<T>, P>>;
        createResponses?: Partial<
            CRUDCreateResponses<T, ChiselEntityClass<T>, P>
        >;
        defaultCreateResponse?: CRUDCreateResponse;
        parsePath?: (url: URL) => P;
    },
): (req: Request) => Promise<Response> {
    const pathTemplateRaw = "/:chiselVersion" + requestContext.path + "/" +
        (urlTemplateSuffix.includes(":id")
            ? urlTemplateSuffix
            : `${urlTemplateSuffix}/:id`);

    const pathTemplate = pathTemplateRaw.replace(/\/+/g, "/"); // in case we end up with foo///bar somehow.

    const defaultCreateResponse = config?.defaultCreateResponse ||
        responseFromJson;
    const parsePath = config?.parsePath ||
        createURLPathParser(pathTemplate);
    const localDefaultCrudMethods =
        defaultCrudMethods as unknown as CRUDMethods<T, E, P>;
    const methods = config?.customMethods
        ? { ...localDefaultCrudMethods, ...config?.customMethods }
        : localDefaultCrudMethods;

    return (req: Request): Promise<Response> => {
        const methodName = req.method as keyof typeof methods; // assume valid, will be handled gracefully
        const createResponse = config?.createResponses?.[methodName] ||
            defaultCreateResponse;
        const method = methods[methodName];
        if (!method) {
            return Promise.resolve(
                createResponse(`Unsupported HTTP method: ${methodName}`, 405),
            );
        }

        const url = new URL(req.url);
        const params = parsePath(url);
        return method(entity, req, params, url, createResponse);
    };
}
// TODO: END: this should be in another file: crud.ts
