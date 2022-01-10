// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

/// <reference path="./dts/lib.deno_core.d.ts" />
/// <reference lib="dom" />

// In the beginning, we shall implement the following querying logic (with the sole exception of the lambdas,
// which can be replaced by simple Attribute compare logic):
//
// select(ChiselIterator<T>, ChiselIterator<T>::Attribute attributes...) -> ChiselIterator<attributes...>
// findMany(ChiselIterator<T>, fn(T)->bool) -> ChiselIterator<T>
// sort(ChiselIterator<T>, fn(T)->Sortable) -> ChiselIterator<T>
// take(ChiselIterator<T>, int) -> ChiselIterator<T>  (takes first n rows)
// join(ChiselIterator<T>, ChiselIterator<U>, ChiselIterator<T>::Attribute, ChiselIterator<U>::Attribute) -> ChiselIterator<Composite<T, U>> (Joins chiselIterators T and U, based on their columns ChiselIterator<T>::Attribute and ChiselIterator<U>::Attribute)
// left_join(ChiselIterator<T>, ChiselIterator<U>, ChiselIterator<T>::Attribute, ChiselIterator<U>::Attribute) -> ChiselIterator<Composite<T, Option<U>>>
// right_join(ChiselIterator<T>, ChiselIterator<U>, ChiselIterator<T>::Attribute, ChiselIterator<U>::Attribute) -> ChiselIterator<Composite<Option<T>, U>>
// transform(ChiselIterator<T>, fn(T)->U)->ChiselIterator<U> (ambitious, maybe later)
//
// Where ChiselIterator<T>::Attribute represents attribute (field) of type (table) T.

type column = [string, string]; // name and type

class Base {
    limit?: number;
    constructor(public columns: column[]) {}
}

// This represents a selection of some columns of a table in a DB.
class BackingStore extends Base {
    // The kind member is use to implement fully covered switch statements.
    readonly kind = "BackingStore";
    constructor(columns: column[], public name: string) {
        super(columns);
    }
}

// This represents an inner join between two chiselIterators.
// FIXME: Add support for ON.
class Join extends Base {
    readonly kind = "Join";
    constructor(
        columns: column[],
        public left: Inner,
        public right: Inner,
    ) {
        super(columns);
    }
}

class Filter extends Base {
    readonly kind = "Filter";
    constructor(
        columns: column[],
        public restrictions: Record<string, unknown>,
        public inner: Inner,
    ) {
        super(columns);
    }
}

type Inner = BackingStore | Join | Filter;

/// XXX: If you add methods here, you also have to add static versions to ChiselEntity (in this file),
/** ChiselIterator is a lazy iterator that will be used by ChiselStrike to construct an optimized query. */
export class ChiselIterator<T> {
    constructor(private type: { new (): T }, private inner: Inner) {}
    /** Force ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    select(...columns: (keyof T)[]): ChiselIterator<Pick<T, (keyof T)>> {
        const names = columns as string[];
        const cs = this.inner.columns.filter((c) => names.includes(c[0]));
        switch (this.inner.kind) {
            case "BackingStore": {
                const b = new BackingStore(cs, this.type.name);
                return new ChiselIterator<T>(undefined, b);
            }
            case "Join": {
                const i = new Join(cs, this.inner.left, this.inner.right);
                return new ChiselIterator(undefined, i);
            }
            case "Filter": {
                const i = new Filter(
                    cs,
                    this.inner.restrictions,
                    this.inner.inner,
                );
                return new ChiselIterator(undefined, i);
            }
        }
    }

    /** Restricts this iterator to contain only at most `limit_` elements */
    take(limit_: number): ChiselIterator<T> {
        const limit = (this.inner.limit == null)
            ? limit_
            : Math.min(limit_, this.inner.limit);

        // shallow copy okay because this is an array of strings
        const cs = [...this.inner.columns];
        // FIXME: refactor to use the same path as select
        switch (this.inner.kind) {
            case "BackingStore": {
                const i = new BackingStore(cs, this.inner.name);
                i.limit = limit;
                return new ChiselIterator(this.type, i);
            }
            case "Join": {
                const i = new Join(cs, this.inner.left, this.inner.right);
                i.limit = limit;
                return new ChiselIterator(this.type, i);
            }
            case "Filter": {
                const i = new Filter(
                    cs,
                    this.inner.restrictions,
                    this.inner.inner,
                );
                i.limit = limit;
                return new ChiselIterator(this.type, i);
            }
        }
    }

    /** Restricts this iterator to contain just the objects that match the `Partial` object `restrictions`. */
    findMany(restrictions: Partial<T>): ChiselIterator<T> {
        const i = new Filter(this.inner.columns, restrictions, this.inner);
        return new ChiselIterator(this.type, i);
    }

    /** Returns a single object that matches the `Partial` object `restrictions` passed as its parameter.
     *
     * If more than one match is found, any is returned. */
    async findOne(restrictions: Partial<T>): Promise<T | null> {
        const i = new Filter(this.inner.columns, restrictions, this.inner);
        const chiselIterator = new ChiselIterator(this.type, i);
        chiselIterator.inner.limit = 1;
        for await (const t of chiselIterator) {
            return t;
        }
        return undefined;
    }

    /** Joins two ChiselIterators, by matching on the properties of the elements in their iterators. */
    join<U>(right: ChiselIterator<U>) {
        const s = new Set();
        const columns = [];
        for (const c of this.inner.columns.concat(right.inner.columns)) {
            if (s.has(c[0])) {
                continue;
            }
            s.add(c[0]);
            columns.push(c);
        }
        const i = new Join(columns, this.inner, right.inner);
        return new ChiselIterator<T & U>(undefined, i);
    }

    /** Executes the function `func` for each element of this iterator. */
    async forEach(func: (arg: T) => void): Promise<void> {
        for await (const t of this) {
            func(t);
        }
    }

    /** Converts this iterator to an Array.
     *
     * Use this with caution as the result set can be very big.
     * It is recommended that you take() first to cap the maximum number of elements. */
    async toArray(): Promise<Partial<T>[]> {
        const arr = new Array<Partial<T>>();
        for await (const t of this) {
            arr.push(t);
        }
        return arr;
    }

    /** ChiselIterator implements asyncIterator, meaning you can use it in any asynchronous context. */
    [Symbol.asyncIterator]() {
        const rid = Deno.core.opSync(
            "chisel_relational_query_create",
            this.inner,
        );
        const ctor = this.type;
        return {
            async next() {
                const properties = await Deno.core.opAsync(
                    "chisel_relational_query_next",
                    rid,
                );
                if (properties) {
                    if (ctor) {
                        const result = new ctor();
                        Object.assign(result, properties);
                        return { value: result, done: false };
                    } else {
                        return { value: properties, done: false };
                    }
                } else {
                    Deno.core.opSync("op_close", rid);
                    return { done: true };
                }
            },
            return() {
                Deno.core.opSync("op_close", rid);
                return { value: undefined as T, done: true };
            },
        };
    }
}

export function chiselIterator<T>(type: { new (): T }, c?: column[]) {
    const columns = (c != undefined)
        ? c
        : Deno.core.opSync("chisel_introspect", { "name": type.name });
    const b = new BackingStore(columns, type.name);
    return new ChiselIterator<T>(type, b);
}

/// XXX: If you add methods here, you also have to add non-static versions nto ChiselIterator (in this file),
/** ChiselEntity is a class that ChiselStrike user-defined entities are expected to extend.
 *
 * It provides properties that are inherent to a ChiselStrike entity, like an id, and static
 * methods that can be used to obtain a `ChiselIterator`.
 */
export class ChiselEntity {
    /** UUID identifying this object. */
    id: string;

    /**
     * Builds a new entity.
     *
     * @param properties The properties of the created entity.
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
     * // now optionally save them to the backend
     * await user.save();
     * await anotherUser.save();
     * ```
     * @returns The persisted entity with given properties and the `id` property set.
     */
    static build<T extends ChiselEntity>(
        this: { new (): T },
        properties: Record<string, unknown>,
    ): T {
        const result = new this();
        Object.assign(result, properties);
        return result;
    }

    /** saves the current object into the backend */
    async save() {
        const jsonIds = await Deno.core.opAsync("chisel_store", {
            name: this.constructor.name,
            value: this,
        });
        function backfillIds(this_: ChiselEntity, jsonIds: unknown) {
            for (const [fieldName, value] of Object.entries(jsonIds)) {
                if (fieldName == "id") {
                    this_.id = value as string;
                } else {
                    backfillIds(this_[fieldName], value);
                }
            }
        }
        backfillIds(this, jsonIds);
    }

    /** Returns a `ChiselIterator` containing all elements of type T known to ChiselStrike.
     *
     * Note that `ChiselIterator` is a lazy iterator, so this doesn't mean a query will be generating fetching all elements at this point. */
    static all<T>(
        this: { new (...arg: Record<string, unknown>[]): T },
    ): ChiselIterator<T> {
        return chiselIterator<T>(this);
    }

    /** Restricts this iterator to contain just the objects that match the `Partial` object `restrictions`. */
    static findMany<T>(
        this: { new (...arg: Record<string, unknown>[]): T },
        restrictions: Partial<T>,
    ): ChiselIterator<T> {
        const it = chiselIterator<T>(this);
        return it.findMany(restrictions);
    }

    /** Restricts this iterator to contain only at most `limit_` elements. */
    static take<T extends ChiselEntity>(
        this: { new (...arg: Record<string, unknown>[]): T },
        limit: number,
    ): ChiselIterator<T> {
        const it = chiselIterator<T>(this);
        return it.take(limit);
    }

    /** Returns a single object that matches the `Partial` object `restrictions` passed as its parameter.
     *
     * If more than one match is found, any is returned. */
    static findOne<T extends ChiselEntity>(
        this: { new (...arg: Record<string, unknown>[]): T },
        restrictions: Partial<T>,
    ): Promise<T | null> {
        const it = chiselIterator<T>(this);
        return it.findOne(restrictions);
    }

    /** Returns an iterator containing all elements of type T known to ChiselStrike,
     * except it also forces ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    static select<T extends ChiselEntity>(
        this: { new (): T },
        ...columns: (keyof T)[]
    ): ChiselIterator<T>;
    static select<T extends ChiselEntity>(
        this: { new (...arg: Record<string, unknown>[]): T },
        ...columns: (keyof T)[]
    ): ChiselIterator<T> {
        const it = chiselIterator<T>(this);
        return it.select(...columns);
    }
}

export class OAuthUser extends ChiselEntity {
    username: string;
}

export const Chisel = {
    api: {
        ChiselIterator: ChiselIterator,
        chiselIterator: chiselIterator,
    },

    buildReadableStreamForBody: function (rid: number) {
        return new ReadableStream({
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
    },

    json: function (body: unknown, status = 200) {
        return new Response(JSON.stringify(body), {
            status: status,
            headers: [
                ["content-type", "application/json"],
            ],
        });
    },
};

/* eslint-disable @typescript-eslint/no-unused-vars */
// deno-lint-ignore-file
export function labels(..._val: string[]) {
    return <T>(_target: T, _propertyName: string) => {
        // chisel-decorator, no content
    };
}

(globalThis as unknown as { Chisel: typeof Chisel }).Chisel = Chisel;
(globalThis as unknown as { ChiselEntity: typeof ChiselEntity }).ChiselEntity =
    ChiselEntity;
(globalThis as unknown as { OAuthUser: typeof OAuthUser }).OAuthUser =
    OAuthUser;
