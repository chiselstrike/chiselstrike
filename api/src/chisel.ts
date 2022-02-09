// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

/// <reference types="./lib.deno_core.d.ts" />
/// <reference lib="dom" />

// In the beginning, we shall implement the following querying logic (with the sole exception of the lambdas,
// which can be replaced by simple Attribute compare logic):
//
// select(ChiselCursor<T>, ChiselCursor<T>::Attribute attributes...) -> ChiselCursor<attributes...>
// filter(ChiselCursor<T>, fn(T)->bool) -> ChiselCursor<T>
// sort(ChiselCursor<T>, fn(T)->Sortable) -> ChiselCursor<T>
// take(ChiselCursor<T>, int) -> ChiselCursor<T>  (takes first n rows)
// join(ChiselCursor<T>, ChiselCursor<U>, ChiselCursor<T>::Attribute, ChiselCursor<U>::Attribute) -> ChiselCursor<Composite<T, U>> (Joins chiselIterators T and U, based on their columns ChiselCursor<T>::Attribute and ChiselCursor<U>::Attribute)
// left_join(ChiselCursor<T>, ChiselCursor<U>, ChiselCursor<T>::Attribute, ChiselCursor<U>::Attribute) -> ChiselCursor<Composite<T, Option<U>>>
// right_join(ChiselCursor<T>, ChiselCursor<U>, ChiselCursor<T>::Attribute, ChiselCursor<U>::Attribute) -> ChiselCursor<Composite<Option<T>, U>>
// transform(ChiselCursor<T>, fn(T)->U)->ChiselCursor<U> (ambitious, maybe later)
//
// Where ChiselCursor<T>::Attribute represents attribute (field) of type (table) T.

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

/** ChiselCursor is a lazy iterator that will be used by ChiselStrike to construct an optimized query. */
export class ChiselCursor<T> {
    constructor(
        private type: { new (): T } | undefined,
        private inner: Inner,
    ) {}
    /** Force ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    select(...columns: (keyof T)[]): ChiselCursor<Pick<T, (keyof T)>> {
        const names = columns as string[];
        const cs = this.inner.columns.filter((c) => names.includes(c[0]));
        switch (this.inner.kind) {
            case "BackingStore": {
                const b = new BackingStore(cs, this.inner.name);
                return new ChiselCursor<T>(undefined, b);
            }
            case "Join": {
                const i = new Join(cs, this.inner.left, this.inner.right);
                return new ChiselCursor(undefined, i);
            }
            case "Filter": {
                const i = new Filter(
                    cs,
                    this.inner.restrictions,
                    this.inner.inner,
                );
                return new ChiselCursor(undefined, i);
            }
        }
    }

    /** Restricts this cursor to contain only at most `limit_` elements */
    take(limit_: number): ChiselCursor<T> {
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
                return new ChiselCursor(this.type, i);
            }
            case "Join": {
                const i = new Join(cs, this.inner.left, this.inner.right);
                i.limit = limit;
                return new ChiselCursor(this.type, i);
            }
            case "Filter": {
                const i = new Filter(
                    cs,
                    this.inner.restrictions,
                    this.inner.inner,
                );
                i.limit = limit;
                return new ChiselCursor(this.type, i);
            }
        }
    }

    /** Restricts this cursor to contain just the objects that match the `Partial` object `restrictions`. */
    filter(restrictions: Partial<T>): ChiselCursor<T> {
        const i = new Filter(this.inner.columns, restrictions, this.inner);
        return new ChiselCursor(this.type, i);
    }

    /** Joins two ChiselCursors, by matching on the properties of the elements in their cursors. */
    join<U>(right: ChiselCursor<U>) {
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
        return new ChiselCursor<T & U>(undefined, i);
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
    [Symbol.asyncIterator]() {
        const rid = Deno.core.opSync(
            "chisel_relational_query_create",
            this.inner,
        );
        const ctor = this.type;
        return {
            async next(): Promise<{ value: T; done: false } | { done: true }> {
                const properties = await Deno.core.opAsync(
                    "chisel_relational_query_next",
                    rid,
                );
                if (properties) {
                    if (ctor) {
                        const result = new ctor();
                        Object.assign(result, properties[0]);
                        return { value: result, done: false };
                    } else {
                        return { value: properties[0], done: false };
                    }
                } else {
                    Deno.core.opSync("op_close", rid);
                    return { done: true };
                }
            },
            return(): { value: T; done: false } | { done: true } {
                Deno.core.opSync("op_close", rid);
                return { done: true };
            },
        };
    }
}

export function chiselIterator<T>(type: { new (): T }, c?: column[]) {
    const columns = (c != undefined)
        ? c
        : Deno.core.opSync("chisel_introspect", { "name": type.name });
    const b = new BackingStore(columns, type.name);
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
     * Maps an entity type to a enumerable of every entity of that type.
     */
    static every<T extends ChiselEntity>(
        this: ObjectType<T>,
    ): Enumerable<T, readonly [T]> {
        return new Enumerable<T, readonly [T]>(this, []);
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

/** Returns the currently logged-in user or null if no one is logged in. */
export async function loggedInUser(): Promise<OAuthUser | null> {
    const id = await Deno.core.opAsync("chisel_user", {});
    return id == null ? null : await OAuthUser.findOne({ id });
}

type ObjectType<T> = { new (): T };
type ValueOf<T extends readonly unknown[]> = T[number];
type ProjectTuple<Args> = Args extends readonly [infer T] ? T : Args;
type Tuplify<T> = T extends readonly unknown[] ? T : readonly [T];

enum OpType {
    Take = "Take",
    Filter = "Filter",
    Map = "Map",
    Sort = "Sort",
}

/**
 * Base class for various Operators applicable on `Enumerable`. Each operator
 * should extend this class and pass on its type identifier from the `OpType`
 * enum.
 */
class Operator {
    constructor(public readonly type: OpType) {}
}

/**
 * Take operator takes first `count` elements from Enumerable collection.
 * The rest is ignored.
 */
class TakeOp extends Operator {
    constructor(public readonly count: number) {
        super(OpType.Take);
    }
}

/**
 * Filter operator applies `predicate` on each element of Enumerable collection
 * and keeps only the elements for which the predicate returns true. The rest
 * is ignored.
 */
class FilterOp<ArgTypes extends readonly unknown[]> extends Operator {
    constructor(
        public readonly predicate: (...args: [...ArgTypes]) => boolean,
    ) {
        super(OpType.Filter);
    }
}

/**
 * Sort operator sorts elements of Enumerable collection using `comparator` which
 * for two elements of the enumerable returns
 *     -1 if `lhs` is considered less than `rhs`
 *     1 if `lhs` is considered greater than `rhs`
 *     0 otherwise
 */
class SortOp<ArgTypes extends readonly unknown[]> extends Operator {
    constructor(
        public readonly comparator: (
            lhs: ProjectTuple<ArgTypes>,
            rhs: ProjectTuple<ArgTypes>,
        ) => number,
    ) {
        super(OpType.Sort);
    }
}

/**
 * Map operator applies provided `map` function to every element of Enumerable collection
 * creating a new Enumerable populated with the results of calling the provided `map` function
 * on every element of the original Enumerable collection.
 */
class MapOp<ArgTypes extends readonly unknown[]> extends Operator {
    constructor(
        public readonly map: (
            ...args: [...ArgTypes]
        ) => unknown | readonly unknown[],
    ) {
        super(OpType.Map);
    }
}

/**
 * An enumerable represents a collection of object tuples transformed from Entity.
 * `OutputTypes` is a tuple of types representing the current output value type of
 * the Enumerable. It is the type of elements you would get if you converted
 * the Enumerable to array using `toArray` method or iterated using async iterator.
 * `BaseEntity` is Entity that was the first Entity used in the call chain.
 */
export class Enumerable<
    BaseEntity extends ChiselEntity,
    OutputTypes extends readonly unknown[],
> {
    constructor(
        private readonly baseEntityCtor: new () => BaseEntity,
        private readonly ops: readonly Operator[],
    ) {}
    /**
     * Filter returns an enumerable that contains all the elements that match the given @predicate.
     */
    filter(
        predicate: (...args: [...OutputTypes]) => boolean,
    ): Enumerable<BaseEntity, OutputTypes> {
        const ops: Operator[] = [...this.ops, new FilterOp(predicate)];
        return new Enumerable<BaseEntity, OutputTypes>(
            this.baseEntityCtor,
            ops,
        );
    }
    /**
     * Sort elements using compare function.
     */
    sort(
        cmp: (
            lhs: ProjectTuple<OutputTypes>,
            rhs: ProjectTuple<OutputTypes>,
        ) => number,
    ): Enumerable<BaseEntity, OutputTypes> {
        const ops: Operator[] = [...this.ops, new SortOp(cmp)];
        return new Enumerable<BaseEntity, OutputTypes>(
            this.baseEntityCtor,
            ops,
        );
    }
    /**
     * Take first `n` elements, discard the rest.
     */
    take(n: number): Enumerable<BaseEntity, OutputTypes> {
        const ops: Operator[] = [...this.ops, new TakeOp(n)];
        return new Enumerable<BaseEntity, OutputTypes>(
            this.baseEntityCtor,
            ops,
        );
    }
    /**
     * Maps the entities of this enumeration to another enumerable of different OutputTypes type.
     */
    map<NewValues>(
        map: (...args: [...OutputTypes]) => NewValues,
    ): Enumerable<BaseEntity, Tuplify<NewValues>> {
        const ops: Operator[] = [...this.ops, new MapOp(map)];
        return new Enumerable<BaseEntity, Tuplify<NewValues>>(
            this.baseEntityCtor,
            ops,
        );
    }

    /**
     * Reduces all elements of this enumerable to a value.
     */
    async reduce(
        fn: (
            previousValue: ProjectTuple<OutputTypes>,
            currentValue: ProjectTuple<OutputTypes>,
        ) => ProjectTuple<OutputTypes>,
    ): Promise<undefined | ProjectTuple<OutputTypes>> {
        let result;
        for await (const current of this) {
            if (result === undefined) {
                result = current;
            } else {
                result = fn(result, current);
            }
        }
        return result;
    }
    /**
     * Iterate over all the elements in this enumerable.
     */
    async forEach(fn: (...args: [...OutputTypes]) => void): Promise<void> {
        const iter = this.makeIterable();
        for await (const val of iter) {
            fn(...val);
        }
    }
    /**
     * Convert this enumerable into an array.
     *
     * Be careful not to convert an enumerable with a lot of elements into an array!
     */
    async toArray(): Promise<ProjectTuple<OutputTypes>[]> {
        const arr = [];
        for await (const val of this) {
            arr.push(val);
        }
        return arr;
    }

    /**
     * Enumerable implements asyncIterator, meaning you can use it in any asynchronous context.
     * The iterator will yield values of type `OutputType`. If `OutputType` is a
     * single-element tuple [T], it will yield just T for convenience.
     */
    async *[Symbol.asyncIterator]() {
        const iter = this.makeIterable();
        for await (const args of iter) {
            // Convenience unwrapping for single-element value tuples.
            if (args.length == 1) {
                yield args[0] as ProjectTuple<OutputTypes>;
            } else {
                yield args as ProjectTuple<OutputTypes>;
            }
        }
    }

    private makeIterable(): AsyncIterable<OutputTypes> {
        const [query, ctors, transforms] = this.prepareEvaluation();
        const rid = Deno.core.opSync(
            "chisel_relational_query_create",
            query,
        );

        let iter: AsyncIterable<readonly unknown[]> = {
            [Symbol.asyncIterator]: async function* () {
                try {
                    while (true) {
                        const propertiesTuple = await queryFetchNext(rid);
                        if (propertiesTuple == undefined) {
                            break;
                        }
                        if (ctors.length != propertiesTuple.length) {
                            throw new Error(
                                "Internal error: constructor and property count mismatch, please file a bug report.",
                            );
                        }

                        const results = [];
                        const zipped = ctors.map((e, i) => {
                            return [e, propertiesTuple[i]] as const;
                        });
                        for (const [ctor, propertyMap] of zipped) {
                            const r = new ctor();
                            Object.assign(r, propertyMap);
                            results.push(r);
                        }
                        yield results;
                    }
                } finally {
                    Deno.core.opSync("op_close", rid);
                }
            },
        };

        for (const trans of transforms) {
            iter = trans.applyIter(iter);
        }
        return iter as AsyncIterable<OutputTypes>;
    }

    private baseEntity(): string {
        return this.baseEntityCtor.name;
    }

    /**
     * `prepareEvaluation` considers `this` Enumerable and prepares backend
     * JSON query and consequent TypeScript transformations.
     * This is done in a few steps. First we consider all operations applied
     * to `this` Enumerable and find a first operation that is not sendable
     * to the backend for evaluation directly in the database (`split` index).
     *
     * Everything up to a split we call `convertibleOps`. The remaining operations
     * get sorted into either `transforms`, which get applied after we get data
     * from the database, or into `convertibleOps`, if it's a Take.
     */
    private prepareEvaluation(): readonly [
        Record<string, unknown>,
        (new () => ChiselEntity)[],
        Transformation[],
    ] {
        const idx = this.ops.findIndex((op) => {
            return op.type != OpType.Take;
        });
        const split = idx != -1 ? idx : this.ops.length;
        const convertibleOps = this.ops.slice(0, split);
        const transforms: Transformation[] = [];
        const constructors: (new () => ChiselEntity)[] = [
            this.baseEntityCtor,
        ];

        for (const op of this.ops.slice(split)) {
            transforms.push(new Transformation(op));
        }

        const query = {
            "version": "v2",
            "base_entity": this.baseEntity(),
            "operations": convertibleOps,
        };
        return [query, constructors, transforms] as const;
    }
}

async function queryFetchNext(
    rid: unknown,
): Promise<undefined | readonly Record<string, unknown>[]> {
    return await Deno.core.opAsync("chisel_relational_query_next", rid);
}

/**
 * Transformation is a helper class used to apply an Operator `op`
 * on an `AsyncIterable` stream of tuples while using `ignoredArgs`
 * count to ignore resp. bypass the last `ignoredArgs` elements of the tuple
 * for operators like `Filter` resp. `Map`.
 */
class Transformation {
    private readonly op: Operator;
    ignoredArgs: number;

    constructor(op: Operator) {
        this.op = op;
        this.ignoredArgs = 0;
    }

    applyIter(
        iter: AsyncIterable<readonly unknown[]>,
    ): AsyncIterable<readonly unknown[]> {
        switch (this.op.type) {
            case OpType.Take: {
                return this.applyTake(this.op as TakeOp, iter);
            }
            case OpType.Filter: {
                return this.applyFilter(
                    this.op as FilterOp<readonly unknown[]>,
                    iter,
                );
            }
            case OpType.Map: {
                return this.applyMap(
                    this.op as MapOp<readonly unknown[]>,
                    iter,
                );
            }
            case OpType.Sort: {
                return this.applySort(
                    this.op as SortOp<readonly unknown[]>,
                    iter,
                );
            }
            default: {
                throw new Error(
                    `can't apply transformation operator of type '${this.op.type}'`,
                );
            }
        }
    }

    private applyTake(
        takeOp: TakeOp,
        iter: AsyncIterable<readonly unknown[]>,
    ): AsyncIterable<readonly unknown[]> {
        return {
            [Symbol.asyncIterator]: async function* () {
                if (takeOp.count == 0) {
                    return;
                }
                let i = 0;
                for await (const e of iter) {
                    yield e;
                    if (++i >= takeOp.count) {
                        break;
                    }
                }
            },
        };
    }

    private applyFilter(
        filterOp: FilterOp<readonly unknown[]>,
        iter: AsyncIterable<readonly unknown[]>,
    ): AsyncIterable<readonly unknown[]> {
        const ignoredArgs = this.ignoredArgs;
        return {
            [Symbol.asyncIterator]: async function* () {
                for await (const args of iter) {
                    if (
                        filterOp.predicate(
                            ...args.slice(0, args.length - ignoredArgs),
                        )
                    ) {
                        yield args;
                    }
                }
            },
        };
    }

    private applyMap(
        mapOp: MapOp<readonly unknown[]>,
        iter: AsyncIterable<readonly unknown[]>,
    ): AsyncIterable<readonly unknown[]> {
        const ignoredArgs = this.ignoredArgs;
        return {
            [Symbol.asyncIterator]: async function* () {
                for await (const args of iter) {
                    const split = args.length - ignoredArgs;
                    const val = mapOp.map(...args.slice(0, split));
                    if (Array.isArray(val)) {
                        yield [...val, ...args.slice(split)];
                    } else {
                        yield [val, ...args.slice(split)];
                    }
                }
            },
        };
    }

    private applySort(
        sortOp: SortOp<readonly unknown[]>,
        iter: AsyncIterable<readonly unknown[]>,
    ): AsyncIterable<readonly unknown[]> {
        const ignoredArgs = this.ignoredArgs;
        return {
            [Symbol.asyncIterator]: async function* () {
                let elements = [];
                for await (const e of iter) {
                    elements.push(e);
                }
                elements = elements.sort((lhs, rhs) => {
                    const split = lhs.length - ignoredArgs;
                    return sortOp.comparator(
                        lhs.slice(0, split),
                        rhs.slice(0, split),
                    );
                });
                for (const e of elements) {
                    yield e;
                }
            },
        };
    }
}
