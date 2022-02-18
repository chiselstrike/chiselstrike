// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

/// <reference types="./lib.deno_core.d.ts" />
/// <reference lib="dom" />

enum OpType {
    BaseEntity = "BaseEntity",
    Take = "Take",
    ColumnsSelect = "ColumnsSelect",
    RestrictionFilter = "RestrictionFilter",
    PredicateFilter = "PredicateFilter",
}

/**
 * Base class for various Operators applicable on `ChiselCursor`. Each operator
 * should extend this class and pass on its `type` identifier from the `OpType`
 * enum.
 */
abstract class Operator {
    constructor(
        public readonly type: OpType,
        public readonly inner: Operator | undefined,
    ) {}

    /** Applies specified Operator `op` on each element of passed iterable
     * `iter` creating a new iterable.
     */
    public abstract apply(
        iter: AsyncIterable<Record<string, unknown>>,
    ): AsyncIterable<Record<string, unknown>>;
}

/**
 * Specifies Entity whose elements are to be fetched.
 */
class BaseEntity extends Operator {
    constructor(
        public name: string,
    ) {
        super(OpType.BaseEntity, undefined);
    }

    apply(
        _iter: AsyncIterable<Record<string, unknown>>,
    ): AsyncIterable<Record<string, unknown>> {
        throw new Error("can't apply BaseEntity operator on an iterable");
    }
}

/**
 * Take operator takes first `count` elements from a collection.
 * The rest is ignored.
 */
class Take extends Operator {
    constructor(
        public readonly count: number,
        inner: Operator,
    ) {
        super(OpType.Take, inner);
    }

    apply(
        iter: AsyncIterable<Record<string, unknown>>,
    ): AsyncIterable<Record<string, unknown>> {
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
 * Forces fetch of just the `columns` (fields) of a given entity.
 */
class ColumnsSelect extends Operator {
    constructor(
        public columns: string[],
        inner: Operator,
    ) {
        super(OpType.ColumnsSelect, inner);
    }

    apply(
        iter: AsyncIterable<Record<string, unknown>>,
    ): AsyncIterable<Record<string, unknown>> {
        const columns = this.columns;
        return {
            [Symbol.asyncIterator]: async function* () {
                for await (const arg of iter) {
                    const newObj: Record<string, unknown> = {};
                    for (const key of columns) {
                        if (arg[key] !== undefined) {
                            newObj[key] = arg[key];
                        }
                    }
                    yield newObj;
                }
            },
        };
    }
}

/**
 * PredicateFilter operator applies @predicate on each element and keeps
 * only those for which the @predicate returns true.
 */
class PredicateFilter extends Operator {
    constructor(
        public predicate: (arg: unknown) => boolean,
        inner: Operator,
    ) {
        super(OpType.PredicateFilter, inner);
    }

    apply(
        iter: AsyncIterable<Record<string, unknown>>,
    ): AsyncIterable<Record<string, unknown>> {
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
 * RestrictionFilter operator applies `restrictions` on each element
 * and keeps only those where field value of a field, specified
 * by restriction key, equals to restriction value.
 */
class RestrictionFilter extends Operator {
    constructor(
        public restrictions: Record<string, unknown>,
        inner: Operator,
    ) {
        super(OpType.RestrictionFilter, inner);
    }

    apply(
        iter: AsyncIterable<Record<string, unknown>>,
    ): AsyncIterable<Record<string, unknown>> {
        const restrictions = Object.entries(this.restrictions);
        return {
            [Symbol.asyncIterator]: async function* () {
                for await (const arg of iter) {
                    verifyMatch: {
                        for (const [key, value] of restrictions) {
                            if (arg[key] != value) {
                                break verifyMatch;
                            }
                        }
                        yield arg;
                    }
                }
            },
        };
    }
}

/** ChiselCursor is a lazy iterator that will be used by ChiselStrike to construct an optimized query. */
export class ChiselCursor<T> {
    constructor(
        private baseConstructor: { new (): T },
        private inner: Operator,
    ) {}
    /** Force ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    select(...columns: (keyof T)[]): ChiselCursor<Pick<T, (keyof T)>> {
        return new ChiselCursor(
            this.baseConstructor,
            new ColumnsSelect(columns as string[], this.inner),
        );
    }

    /** Restricts this cursor to contain only at most `count` elements */
    take(count: number): ChiselCursor<T> {
        return new ChiselCursor(
            this.baseConstructor,
            new Take(count, this.inner),
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
                    arg1 as ((arg: unknown) => boolean),
                    this.inner,
                ),
            );
        } else {
            return new ChiselCursor(
                this.baseConstructor,
                new RestrictionFilter(arg1, this.inner),
            );
        }
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
    async toArray(): Promise<Partial<T>[]> {
        const arr = [];
        for await (const t of this) {
            arr.push(t);
        }
        return arr;
    }

    /** ChiselCursor implements asyncIterator, meaning you can use it in any asynchronous context. */
    async *[Symbol.asyncIterator]() {
        let iter = this.makeTransformedQueryIter(this.inner);
        if (iter === undefined) {
            iter = this.makeQueryIter(this.inner);
        }
        for await (const it of iter) {
            yield it as T;
        }
    }

    /** Performs recursive descent via Operator.inner examining the whole operator
     * chain. If PredicateFilter is encountered, a backend query is generated and all consecutive
     * operations are applied on the resulting async iterable in TypeScript. In such a
     * case, the function returns the resulting AsyncIterable.
     * If no PredicateFilter is found, undefined is returned.
     */
    private makeTransformedQueryIter(
        op: Operator,
    ): AsyncIterable<Record<string, unknown>> | undefined {
        if (op.type == OpType.BaseEntity) {
            return undefined;
        } else if (op.inner === undefined) {
            throw new Error(
                "internal error: expected inner operator, got undefined",
            );
        }
        let iter = this.makeTransformedQueryIter(op.inner);
        if (iter !== undefined) {
            return op.apply(iter);
        } else if (op.type == OpType.PredicateFilter) {
            iter = this.makeQueryIter(op.inner);
            return op.apply(iter);
        } else {
            return undefined;
        }
    }

    private makeQueryIter(
        op: Operator,
    ): AsyncIterable<Record<string, unknown>> {
        const ctor = this.containsSelect(op) ? undefined : this.baseConstructor;
        return {
            [Symbol.asyncIterator]: async function* () {
                const rid = Deno.core.opSync(
                    "chisel_relational_query_create",
                    op,
                );
                try {
                    while (true) {
                        const properties = await Deno.core.opAsync(
                            "chisel_relational_query_next",
                            rid,
                        );

                        if (properties == undefined) {
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
                    Deno.core.opSync("op_close", rid);
                }
            },
        };
    }

    /** Recursively examines operator chain searching for ColumnsSelect operator.
     * Returns true if found, false otherwise.
     */
    private containsSelect(op: Operator): boolean {
        if (op.type == OpType.ColumnsSelect) {
            return true;
        } else if (op.inner === undefined) {
            return false;
        } else {
            return this.containsSelect(op.inner);
        }
    }
}

export function chiselIterator<T>(type: { new (): T }) {
    const b = new BaseEntity(type.name);
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
        ...properties: Record<string, unknown>[]
    ): T {
        const result = new this();
        Object.assign(result, ...properties);
        return result;
    }

    /** saves the current object into the backend */
    async save() {
        const jsonIds = await Deno.core.opAsync("chisel_store", {
            name: this.constructor.name,
            value: this,
        });
        type IdsJson = Map<string, IdsJson>;
        function backfillIds(this_: ChiselEntity, jsonIds: IdsJson) {
            for (const [fieldName, value] of Object.entries(jsonIds)) {
                if (fieldName == "id") {
                    this_.id = value as string;
                } else {
                    const child = (this_ as unknown as Record<string, unknown>)[
                        fieldName
                    ];
                    backfillIds(child as ChiselEntity, value);
                }
            }
        }
        backfillIds(this, jsonIds);
    }

    /** Returns a `ChiselCursor` containing all elements of type T known to ChiselStrike.
     *
     * Note that `ChiselCursor` is a lazy iterator, so this doesn't mean a query will be generating fetching all elements at this point. */
    static cursor<T>(
        this: { new (): T },
    ): ChiselCursor<T> {
        return chiselIterator<T>(this);
    }

    /** Restricts this iterator to contain just the objects that match the `Partial` object `restrictions`. */
    static async findMany<T>(
        this: { new (): T },
        restrictions: Partial<T>,
        take?: number,
    ): Promise<Partial<T>[]> {
        let it = chiselIterator<T>(this);
        if (take) {
            it = it.take(take);
        }
        return await it.filter(restrictions).toArray();
    }

    /** Returns a single object that matches the `Partial` object `restrictions` passed as its parameter.
     *
     * If more than one match is found, any is returned. */
    static async findOne<T extends ChiselEntity>(
        this: { new (): T },
        restrictions: Partial<T>,
    ): Promise<T | null> {
        const it = chiselIterator<T>(this).filter(restrictions).take(1);
        for await (const value of it) {
            return value;
        }
        return null;
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
        await Deno.core.opAsync("chisel_entity_delete", {
            type_name: this.name,
            restrictions: restrictions,
        });
    }
}

export class OAuthUser extends ChiselEntity {
    username: string | undefined = undefined;
}

export function buildReadableStreamForBody(rid: number) {
    return new ReadableStream<string>({
        async pull(controller: ReadableStreamDefaultController) {
            const chunk = await Deno.core.opAsync("chisel_read_body", rid);
            if (chunk) {
                controller.enqueue(chunk);
            } else {
                controller.close();
                Deno.core.opSync("op_close", rid);
            }
        },
        cancel() {
            Deno.core.opSync("op_close", rid);
        },
    });
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
    const secret = Deno.core.opSync("chisel_get_secret", key);
    if (secret === undefined || secret === null) {
        return undefined;
    }
    return secret;
}

export function responseFromJson(body: unknown, status = 200) {
    // https://fetch.spec.whatwg.org/#null-body-status
    const isNullBody = (status: number): boolean => {
        return status == 101 || status == 204 || status == 205 || status == 304;
    };

    const json = isNullBody(status) ? null : JSON.stringify(body);
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

export function unique(): void {
    // chisel-decorator, no content
}

/** Returns the currently logged-in user or null if no one is logged in. */
export async function loggedInUser(): Promise<OAuthUser | null> {
    const id = await Deno.core.opAsync("chisel_user", {});
    return id == null ? null : await OAuthUser.findOne({ id });
}
