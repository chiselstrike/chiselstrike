/// <reference lib="esnext" />
/// <reference lib="dom" />

/** ChiselIterator is a lazy iterator that will be used by ChiselStrike to construct an optimized query. */
declare class ChiselIterator<T> {
    /** Restricts this iterator to contain just the objects that match the `Partial` object `restrictions`. */
    findMany(restrictions: Partial<T>): ChiselIterator<T>;
    /** Returns a single object that matches the `Partial` object `restrictions` passed as its parameter.
     *
     * If more than one match is found, any is returned. */
    findOne(restrictions: Partial<T>): Promise<T | null>;
    /** Force ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    select(...columns: (keyof T)[]): ChiselIterator<T>;
    /** ChiselIterator implements asyncIterator, meaning you can use it in any asynchronous context. */
    [Symbol.asyncIterator]: () => AsyncIterator<T>;
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

declare type Chisel = {
    /** Saves an object, returning the saved object. In particular, if this is a new element an `id` is populated at this point. */
    save: <T>(typeName: string, content: T) => Promise<T>;
    /** helper function that generates a JSON response from an Object. */
    json: <T>(body: T, status?: number) => Response;
};

/** ChiselEntity is a class that ChiselStrike user-defined entities are expected to extend.
 *
 * It provides properties that are inherent to a ChiselStrike entity, like an id, and static
 * methods that can be used to obtain a `ChiselIterator`.
 */
declare class ChiselEntity {
    /** UUID identifying this object. */
    id: string;
    /** Returns a `ChiselIterator` containing all elements of type T known to ChiselStrike.
     *
     * Note that `ChiselIterator` is a lazy iterator, so this doesn't mean a query will be generating fetching all elements at this point. */
    static all<T>(this: { new (): T }): ChiselIterator<T>;

    /** Restricts this iterator to contain just the objects that match the `Partial` object `restrictions`. */
    static findMany<T>(
        this: { new (): T },
        restrictions: Partial<T>,
    ): ChiselIterator<T>;
    /** Restricts this iterator to contain only at most `limit_` elements. */
    static take<T extends ChiselEntity>(
        this: { new (): T },
        limit: number,
    ): ChiselIterator<T>;
    /** Returns a single object that matches the `Partial` object `restrictions` passed as its parameter.
     *
     * If more than one match is found, any is returned. */
    static findOne<T extends ChiselEntity>(
        this: { new (): T },
        restrictions: Partial<T>,
    ): Promise<T | null>;

    /** Returns an iterator containing all elements of type T known to ChiselStrike,
     * except it also forces ChiselStrike to fetch just the `...columns` that are part of the colums list. */
    static select<T extends ChiselEntity>(
        this: { new (): T },
        ...columns: (keyof T)[]
    ): ChiselIterator<T>;
}

declare const Chisel: Chisel;

declare class OAuthUser extends ChiselEntity {
    username: string;
}
