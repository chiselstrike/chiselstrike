/// <reference lib="esnext" />
/// <reference lib="dom" />

declare class ChiselIterator<T> {
    findMany(restrictions: Partial<T>): ChiselIterator<T>;
    findOne(restrictions: Partial<T>): Promise<T | null>;
    select(...columns: (keyof T)[]): ChiselIterator<T>;
    [Symbol.asyncIterator]: () => AsyncIterator<T>;
    join<U>(right: ChiselIterator<U>): ChiselIterator<T & U>;
    take(limit_: number): ChiselIterator<T>;
    toArray(): Promise<Partial<T>[]>;
    forEach(func: (arg: T) => void): Promise<void>;
}

declare type Chisel = {
    save: <T>(typeName: string, content: T) => Promise<T>;
    json: <T>(body: T, status?: number) => Response;
};

declare class ChiselEntity {
    id: string;
    static all<T>(this: { new (): T }): ChiselIterator<T>;
    static findMany<T>(
        this: { new (): T },
        restrictions: Partial<T>,
    ): ChiselIterator<T>;
    static take<T extends ChiselEntity>(
        this: { new (): T },
        limit: number,
    ): ChiselIterator<T>;
    static findOne<T extends ChiselEntity>(
        this: { new (): T },
        restrictions: Partial<T>,
    ): Promise<T | null>;
    static select<T extends ChiselEntity>(
        this: { new (): T },
        ...columns: (keyof T)[]
    ): ChiselIterator<T>;
}

declare const Chisel: Chisel;
