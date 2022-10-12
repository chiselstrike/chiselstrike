// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { crud } from "./crud.ts";
import type { RouteMap } from "./routing.ts";
import { opAsync, opSync } from "./utils.ts";
import { SimpleTypeSystem, TypeSystem } from "./type_system.ts";
/**
 * Base class for various Operators applicable on `ChiselCursor`. An
 * implementation of Operator<T> processes an AsyncIterable<T> and
 * produces an AsyncIterable<U> for some, implementation defined, U.
 * The base class is generic so that the apply function type is
 * sound. Note that TypeScript *doesn't* check this.
 */
abstract class Operator<Input, Output> {
    // Read by rust
    readonly type: string;
    constructor(
        public readonly inner: Operator<unknown, Input> | undefined,
    ) {
        this.type = this.constructor.name;
    }

    /** Applies specified Operator `op` on each element of passed iterable
     * `iter` creating a new iterable.
     */
    public abstract apply(
        iter: AsyncIterable<Input>,
    ): AsyncIterable<Output>;

    public abstract recordToOutput(rawRecord: unknown): Output;

    public eval(): AsyncIterable<Output> | undefined {
        const iter = this.inner!.eval();
        if (iter !== undefined) {
            return this.apply(iter);
        }
        return undefined;
    }

    public runChiselQuery(): AsyncIterable<Output> {
        const getRid = () =>
            opAsync(
                "op_chisel_relational_query_create",
                this,
                requestContext.rid,
            ) as Promise<number>;
        const recordToOutput = (rawRecord: unknown) => {
            return this.recordToOutput(rawRecord);
        };
        return {
            [Symbol.asyncIterator]: async function* () {
                const rid = await getRid();
                try {
                    while (true) {
                        await opAsync("op_chisel_query_next", rid);
                        const properties = opSync(
                            "op_chisel_query_get_value",
                            rid,
                            requestContext.rid,
                        );

                        if (properties === null) {
                            break;
                        }
                        yield recordToOutput(properties);
                    }
                } finally {
                    Deno.core.close(rid);
                }
            },
        };
    }
}

/**
 * Specifies Entity whose elements are to be fetched.
 */
class BaseEntity<T> extends Operator<never, T> {
    constructor(
        public name: string,
        private baseConstructor: { new (): T },
    ) {
        super(undefined);
    }

    apply(
        _iter: AsyncIterable<never>,
    ): AsyncIterable<T> {
        throw new Error("can't apply BaseEntity operator on an iterable");
    }

    recordToOutput(rawRecord: unknown): T {
        return buildEntity(
            this.baseConstructor,
            rawRecord as Record<string, unknown>,
        );
    }

    public eval(): undefined {
        return undefined;
    }
}

/**
 * Take operator takes first `count` elements from a collection.
 * The rest is ignored.
 */
class Take<T> extends Operator<T, T> {
    constructor(
        inner: Operator<unknown, T>,
        public readonly count: number,
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

    recordToOutput(rawRecord: unknown): T {
        return this.inner!.recordToOutput(rawRecord);
    }
}

/**
 * Skip operator skips first `count` elements from a collection.
 */
class Skip<T> extends Operator<T, T> {
    constructor(
        inner: Operator<unknown, T>,
        public readonly count: number,
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

    recordToOutput(rawRecord: unknown): T {
        return this.inner!.recordToOutput(rawRecord);
    }
}

/**
 * Map operator applies a function to each element of this collection
 */
class MapOperator<Input, Output> extends Operator<Input, Output> {
    constructor(
        inner: Operator<unknown, Input>,
        public func: (arg: Input) => Output,
    ) {
        super(inner);
    }

    apply(
        iter: AsyncIterable<Input>,
    ): AsyncIterable<Output> {
        const func = this.func;
        return {
            [Symbol.asyncIterator]: async function* () {
                for await (const arg of iter) {
                    yield func(arg);
                }
            },
        };
    }

    public eval(): AsyncIterable<Output> {
        let iter = this.inner!.eval();
        if (iter === undefined) {
            iter = this.inner!.runChiselQuery();
        }
        return this.apply(iter);
    }

    recordToOutput(_rawRecord: unknown): Output {
        throw new Error(
            "map operator doesn't get sent to the database, so this code should not have been called!",
        );
    }
}

/**
 * Forces fetch of just the `columns` (fields) of a given entity.
 */
class ColumnsSelect<T, C extends (keyof T)[]>
    extends Operator<T, Pick<T, C[number]>> {
    constructor(
        inner: Operator<unknown, T>,
        public columns: C,
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

    recordToOutput(rawRecord: unknown): T {
        // In this case, we have run a Select operator (which converts T to Pick<T,..>),
        // therefore we are producing a plain Record with selected properties which
        // means we can simply cast and return.
        return rawRecord as T;
    }
}

/**
 * PredicateFilter operator applies `predicate` on each element and keeps
 * only those for which the `predicate` returns true.
 */
class PredicateFilter<T> extends Operator<T, T> {
    constructor(
        inner: Operator<unknown, T>,
        public predicate: (arg: T) => boolean,
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

    public eval(): AsyncIterable<T> {
        let iter = this.inner!.eval();
        if (iter === undefined) {
            iter = this.inner!.runChiselQuery();
        }
        return this.apply(iter);
    }

    recordToOutput(rawRecord: unknown): T {
        return this.inner!.recordToOutput(rawRecord);
    }
}

/**
 * ExpressionFilter operator is intended only to be used by Chisel compiler.
 * It applies `predicate` on each element and keeps only those for which
 * the `predicate` returns true. The Chisel compiler provides an `expression`
 * as well which is to be equivalent to the predicate and which is sent to
 * the Rust backend for direct Database evaluation if possible.
 */
class ExpressionFilter<T> extends Operator<T, T> {
    constructor(
        inner: Operator<unknown, T>,
        public predicate: (arg: T) => boolean,
        public expression: Record<string, unknown>,
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

    recordToOutput(rawRecord: unknown): T {
        return this.inner!.recordToOutput(rawRecord);
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
class SortBy<T> extends Operator<T, T> {
    constructor(
        inner: Operator<unknown, T>,
        private keys: SortKey<T>[],
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

    recordToOutput(rawRecord: unknown): T {
        return this.inner!.recordToOutput(rawRecord);
    }
}

/**
 * AggregateBy operator is an intermediate Operator used to implement various aggregation
 * operators like MinBy/MaxBy. It provides a general aggregate interface performing a fold
 * along `Input[K]` field values using `init` initial value and aggerage (fold) operator
 * `aggregateOp`.
 *
 * This Operator can't be used directly as our Rust backend uses the name of the operator
 * for identification.
 */
abstract class AggregateBy<Input, K extends keyof Input, Output>
    extends Operator<Input, Output> {
    constructor(
        inner: Operator<unknown, Input>,
        private key: K,
        private init: Output,
        private aggregateOp: (lhs: Output, rhs: Input[K]) => Output,
    ) {
        super(inner);
    }
    apply(
        iter: AsyncIterable<Input>,
    ): AsyncIterable<Output> {
        const key = this.key;
        const init = this.init;
        const aggregateOp = this.aggregateOp;
        return {
            [Symbol.asyncIterator]: async function* () {
                let result = init;
                for await (const e of iter) {
                    result = aggregateOp(result, e[key]);
                }
                yield result;
            },
        };
    }

    public eval(): AsyncIterable<Output> {
        let iter = this.inner!.eval();
        if (iter === undefined) {
            iter = this.inner!.runChiselQuery();
        }
        return this.apply(iter);
    }

    recordToOutput(rawRecord: unknown): Output {
        return rawRecord as Output;
    }
}

class MinBy<Input, K extends keyof Input>
    extends AggregateBy<Input, K, Input[K] | undefined> {
    constructor(inner: Operator<unknown, Input>, key: K) {
        const min = (v1: Input[K] | undefined, v2: Input[K]) => {
            if (v2 === undefined || v2 === null) {
                return v1;
            }
            if (v1 === undefined) {
                return v2;
            }
            return v1 < v2 ? v1 : v2;
        };
        super(inner, key, undefined, min);
    }
}

class MaxBy<Input, K extends keyof Input>
    extends AggregateBy<Input, K, Input[K] | undefined> {
    constructor(inner: Operator<unknown, Input>, key: K) {
        const max = (v1: Input[K] | undefined, v2: Input[K]) => {
            if (v2 === undefined || v2 === null) {
                return v1;
            }
            if (v1 === undefined) {
                return v2;
            }
            return v1 > v2 ? v1 : v2;
        };
        super(inner, key, undefined, max);
    }
}

/** ChiselCursor is a lazy iterator that will be used by ChiselStrike to construct an optimized query. */
export class ChiselCursor<T> {
    constructor(private inner: Operator<unknown, T>) {}

    // FIXME: The typing of Select operator is wrong because Pick allows to select
    // not only properties, but methods as well. Which means `Person.cursor().select("save");`
    // will pass the compiler, but it's obviously not what we want to allow.

    /** Force ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    select<C extends (keyof T)[]>(
        ...columns: C
    ): ChiselCursor<Pick<T, C[number]>> {
        return new ChiselCursor(
            new ColumnsSelect<T, (keyof T)[]>(
                this.inner,
                columns,
            ),
        );
    }

    /** Restricts this cursor to contain only at most `count` elements */
    take(count: number): ChiselCursor<T> {
        return new ChiselCursor(
            new Take(this.inner, count),
        );
    }

    /** Skips the first `count` elements of this cursor. */
    skip(count: number): ChiselCursor<T> {
        return new ChiselCursor(
            new Skip(this.inner, count),
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
                new PredicateFilter(
                    this.inner,
                    arg1,
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
                new ExpressionFilter(
                    this.inner,
                    predicate,
                    expr,
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
        let op: Operator<T, T> = new ExpressionFilter(
            this.inner,
            exprPredicate,
            expression,
        );
        if (postPredicate !== undefined) {
            op = new PredicateFilter(op, postPredicate);
        }
        return new ChiselCursor(op);
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
            new SortBy(
                this.inner,
                [new SortKey<T>(key, ascending)],
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
        const c = new ChiselCursor(
            new MinBy<T, K>(this.inner, key),
        );
        for await (const min of c) {
            return min;
        }
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
        const c = new ChiselCursor(
            new MaxBy<T, K>(this.inner, key),
        );
        for await (const max of c) {
            return max;
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
    async toArray(): Promise<T[]> {
        const arr = [];
        for await (const t of this) {
            arr.push(t);
        }
        return arr;
    }

    /**
     * The map() method creates a new cursor populated with the results of calling a provided function on every element in the calling cursor.
     *
     * @param `func` the function to apply. It has as its parameter the original element, and it returns the mapped element
     */
    map<V>(func: (arg: T) => V): ChiselCursor<V> {
        return new ChiselCursor(
            new MapOperator(this.inner, func),
        );
    }

    /** ChiselCursor implements asyncIterator, meaning you can use it in any asynchronous context. */
    [Symbol.asyncIterator](): AsyncIterator<T> {
        let iter = this.inner.eval();
        if (iter === undefined) {
            iter = this.inner.runChiselQuery();
        }
        return iter[Symbol.asyncIterator]();
    }
}

export function chiselIterator<T extends ChiselEntity>(
    type: { new (): T },
) {
    const b = new BaseEntity<T>(type.name, type);
    return new ChiselCursor(b);
}

export type UpsertArgs<T> = {
    restrictions: Partial<T>;
    create: Partial<T>;
    update: Partial<T>;
};

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
        return buildEntity(
            this,
            ...properties,
        );
    }

    /** saves the current object into the backend */
    async save() {
        ensureNotGet();
        type IdsJson = { id: string; children: Record<string, IdsJson> };
        const jsonIds = await opAsync("op_chisel_store", {
            name: this.constructor.name,
            value: this,
        }, requestContext.rid) as IdsJson;
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
     * Returns a single entity of type T that is identified by `entityId`.
     *
     * If no such entity is found, an exception is thrown.
     */
    static async byId<T extends ChiselEntity>(
        this: typeof ChiselEntity & { new (): T },
        entityId: Id<T>,
    ): Promise<T> {
        const entity = await this.findById<T>(entityId);
        if (entity === undefined) {
            throw Error(`failed to find entity with id '${entityId}'`);
        }
        return entity;
    }

    /**
     * Returns a single entity of type T that is identified by `entityId`.
     *
     * If no such entity is found, returns undefined.
     */
    static async findById<T extends ChiselEntity>(
        this: { new (): T },
        entityId: Id<T>,
    ): Promise<T | undefined> {
        const it = chiselIterator<T>(this).filter(
            { id: entityId } as Partial<T>,
        ).take(1);
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
        await opAsync("op_chisel_delete", {
            typeName: this.name,
            filterExpr: restrictionsToFilterExpr(restrictions),
        }, requestContext.rid);
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
     * * **PATCH:**
     *     * `/comments/:id`         modifies the element with the given ID. Payload is a JSON with the properties of `Comment` that will be modified as keys.
     *
     * If you need more control over which method to generate and their behavior, see the top-level `crud()` function
     *
     * @returns A route map suitable as a default export in a route.
     */
    static crud(): RouteMap {
        return crud(this);
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
        const entity = buildEntity(
            this,
            ...properties,
        );
        await entity.save();
        return entity;
    }

    /**
     * Update an object or create it if it doesn't exist.
     *
     * @example
     * ```typescript
     * export class User extends ChiselEntity {
     *   username: string,
     *   email: string,
     * }
     * const user = await User.upsert({
     *     restrictions: { username: "alice", email: "alice@example.com" },
     *     create: { username: "alice", email: "alice@example.com" },
     *     update: { email: "alice@chiselstrike.com" }
     * });
     * ```
     *
     * Please note that upsert only updates a single row it matches on.
     *
     * @version experimental
     */
    static async upsert<T extends ChiselEntity>(
        this: { new (): T },
        args: UpsertArgs<T>,
    ): Promise<T> {
        const it = chiselIterator<T>(this).filter(args.restrictions);
        let foundEntity = null;
        for await (const value of it) {
            foundEntity = value;
            break;
        }
        if (foundEntity) {
            mergeIntoEntity(
                this.name,
                foundEntity as Record<string, unknown>,
                args.update,
            );
            await foundEntity.save();
            return foundEntity;
        } else {
            const entity = buildEntity(
                this,
                args.create,
            );
            await entity.save();
            return entity;
        }
    }
}

function restrictionsToFilterExpr<T extends ChiselEntity>(
    restrictions: Partial<T>,
): Record<string, unknown> | undefined {
    if (typeof restrictions != "object") {
        throw `expected object, but found ${typeof restrictions} instead`;
    }
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
                exprType: "Value",
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

export function labels(..._val: string[]) {
    return <T>(_target: T, _propertyName: string) => {
        // chisel-decorator, no content
    };
}

export function unique(_target: unknown, _name: string): void {
    // chisel-decorator, no content
}

export const requestContext: {
    rid: number | undefined;
    method: string;
    userId: string | undefined;
} = {
    rid: undefined,
    method: "",
    userId: undefined,
};

const typeSystem: TypeSystem = new TypeSystem(
    opSync("op_chisel_get_type_system") as SimpleTypeSystem,
);

function ensureNotGet() {
    if (requestContext.method === "GET") {
        throw new Error("Mutating the backend is not allowed during GET");
    }
}

export class AuthUser extends ChiselEntity {
    emailVerified?: string;
    name?: string;
    email?: string;
    image?: string;
}

/** Returns the currently logged-in user or null if no one is logged in. */
export async function loggedInUser(): Promise<AuthUser | undefined> {
    const id = requestContext.userId;
    if (id === undefined) {
        return undefined;
    }
    return await AuthUser.findOne({ id });
}

function buildEntity<T>(
    baseConstructor: { new (): T },
    ...sources: Record<string, unknown>[]
) {
    const entity = new baseConstructor();
    mergeIntoEntity(
        baseConstructor.name,
        entity as Record<string, unknown>,
        ...sources,
    );
    return entity;
}

export function mergeIntoEntity(
    entityName: string,
    target: Record<string, unknown>,
    ...sources: Record<string, unknown>[]
) {
    for (const source of sources) {
        mergeSourceIntoEntity(entityName, target, source);
    }
}

function mergeSourceIntoEntity(
    entityName: string,
    target: Record<string, unknown>,
    source: Record<string, unknown>,
) {
    const entity = typeSystem.findEntity(entityName);
    if (entity === undefined) {
        throw new Error(
            `trying to build an unknown entity '${entityName}'`,
        );
    }
    for (const field of entity.fields) {
        if (!(field.name in source)) {
            continue;
        }
        const fieldValue = source[field.name];

        // Id needs to be handled specially as it can be undefined before it's saved although it's
        // not marked optional within our type system.
        if (field.name == "id") {
            target[field.name] = fieldValue;
            continue;
        }

        // If there is explicitly provided undefined/null value for a field,
        // check that it's actually optional.
        if (fieldValue === undefined || fieldValue === null) {
            if (field.isOptional) {
                target[field.name] = fieldValue;
                continue;
            } else {
                throw new Error(
                    `field ${field.name} of entity ${entityName} is not optional but undefined/null was explicitly provided for the field`,
                );
            }
        }

        const err = (typeName: string) => {
            return new Error(
                `field ${field.name} of entity ${entityName} is ${typeName}, but provided value ${fieldValue} is of type ${typeof fieldValue}`,
            );
        };
        const typeName = field.type.name;
        if (typeName == "string" || typeName == "entityId") {
            if (typeof fieldValue == "string") {
                target[field.name] = fieldValue;
            } else {
                throw err("string");
            }
        } else if (typeName == "number") {
            if (typeof fieldValue == "number") {
                target[field.name] = fieldValue;
            } else {
                throw err("number");
            }
        } else if (typeName == "boolean") {
            if (typeof fieldValue == "boolean") {
                target[field.name] = fieldValue;
            } else {
                throw err("boolean");
            }
        } else if (typeName == "arrayBuffer") {
            // This covers the CRUD path where we get the value in form of a
            // base64 encoded string.
            if (typeof fieldValue == "string") {
                // TODO: Get rid of this when we have deno imports working.
                target[field.name] = Uint8Array.from(
                    atob(fieldValue),
                    (c) => c.charCodeAt(0),
                );
            } else {
                target[field.name] = fieldValue;
            }
        } else if (typeName == "jsDate") {
            if (fieldValue instanceof Date) {
                target[field.name] = fieldValue;
            } else if (
                typeof fieldValue == "string" || typeof fieldValue == "number"
            ) {
                target[field.name] = new Date(fieldValue);
            } else {
                throw new Error(
                    `field ${field.name} of entity ${entityName} is Date, but provided value ${fieldValue} is not an instance of Date`,
                );
            }
        } else if (typeName == "array") {
            if (Array.isArray(fieldValue)) {
                target[field.name] = fieldValue;
            } else {
                throw new Error(
                    `field ${field.name} of entity ${entityName} is Array, but provided value ${fieldValue} is not an instance of Array`,
                );
            }
        } else if (typeName == "entity") {
            if (typeof fieldValue == "object") {
                if (target[field.name] === undefined) {
                    target[field.name] = new ChiselEntity();
                }
                type RecordType = Record<string, unknown>;
                mergeIntoEntity(
                    field.type.entityName,
                    target[field.name] as RecordType,
                    fieldValue as RecordType,
                );
            } else {
                throw new Error(
                    `field ${field.name} of entity ${entityName} is Entity, but provided value ${fieldValue} is not an object`,
                );
            }
        } else {
            assertNever(typeName);
            throw new Error(
                `field '${field.name}' of entity '${entityName}' has unexpected type '${typeName}'`,
            );
        }
    }
}

function assertNever(_: never) {
    return false;
}

export type Id<Entity extends ChiselEntity> = Entity["id"];
