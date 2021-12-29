/// <reference lib="esnext" />
/// <reference lib="dom" />

/// FIXME: This file is mostly auto generated. It should be created during the build.

declare let OAuthUser: ChiselIterator<{ username: string }>;
declare type column = [string, string];
declare class Base {
    columns: column[];
    limit?: number;
    constructor(columns: column[]);
}
declare class BackingStore extends Base {
    name: string;
    readonly kind = "BackingStore";
    constructor(columns: column[], name: string);
}
declare class Join extends Base {
    left: Inner;
    right: Inner;
    readonly kind = "Join";
    constructor(columns: column[], left: Inner, right: Inner);
}
declare class Filter extends Base {
    restrictions: Record<string, unknown>;
    inner: Inner;
    readonly kind = "Filter";
    constructor(
        columns: column[],
        restrictions: Record<string, unknown>,
        inner: Inner,
    );
}
declare type Inner = BackingStore | Join | Filter;
/** ChiselIterator is a lazy iterator that will be used by ChiselStrike to construct an optimized query. */
declare class ChiselIterator<T> {
    private inner;
    constructor(inner: Inner);
    /** Restricts this iterator to contain just the objects that match the `Partial` object `restrictions`. */
    findMany(restrictions: Partial<T>): ChiselIterator<T>;
    /** Returns a single object that matches the `Partial` object `restrictions` passed as its parameter.
     *
     * If more than one match is found, any is returned. */
    findOne(restrictions: Partial<T>): Promise<T | null>;
    /** Force ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    select(...columns: (keyof T)[]): ChiselIterator<Pick<T, (keyof T)>>;
    /** ChiselIterator implements asyncIterator, meaning you can use it in any asynchronous context. */
    [Symbol.asyncIterator](): {
        next(): Promise<
            {
                value: T;
                done: boolean;
            } | {
                done: boolean;
                value?: undefined;
            }
        >;
        return(): {
            value: T;
            done: boolean;
        };
    };
    /** Joins two ChiselIterators, by matching on the properties of the elements in their iterators. */
    join<U>(right: ChiselIterator<U>): ChiselIterator<T & U>;
    /** Restricts this iterator to contain only at most `limit_` elements */
    take(limit_: number): ChiselIterator<T>;
    /** Converts this iterator to an Array.
     *
     * Use this with caution as the result set can be very big.
     * It is recommended that you take() first to cap the maximum number of elements. */
    toArray(): Promise<Partial<T>[]>;
    /** Executes the function `func` for each element of this iterator. */
    forEach(func: (arg: T) => void): Promise<void>;
}

declare function chiselIterator<T>(
    name: string,
    columns: column[],
): ChiselIterator<T>;

declare namespace Chisel {
    /** Saves an object, returning the saved object. In particular, if this is a new element an `id` is populated at this point. */
    function save<T>(typeName: string, content: T): Promise<T>;
    /** helper function that generates a JSON response from an Object. */
    function json<T>(body: T, status?: number): Response;
}
