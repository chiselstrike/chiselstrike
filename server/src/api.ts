// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

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

export class ChiselIterator<T> {
    constructor(private inner: Inner) {}
    select(...columns: (keyof T)[]): ChiselIterator<Pick<T, (keyof T)>> {
        const names = columns as string[];
        const cs = this.inner.columns.filter((c) => names.includes(c[0]));
        switch (this.inner.kind) {
            case "BackingStore":
                return chiselIterator(this.inner.name, cs);
            case "Join": {
                const i = new Join(cs, this.inner.left, this.inner.right);
                return new ChiselIterator(i);
            }
            case "Filter": {
                const i = new Filter(
                    cs,
                    this.inner.restrictions,
                    this.inner.inner,
                );
                return new ChiselIterator(i);
            }
        }
    }

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
                return new ChiselIterator(i);
            }
            case "Join": {
                const i = new Join(cs, this.inner.left, this.inner.right);
                i.limit = limit;
                return new ChiselIterator(i);
            }
            case "Filter": {
                const i = new Filter(
                    cs,
                    this.inner.restrictions,
                    this.inner.inner,
                );
                i.limit = limit;
                return new ChiselIterator(i);
            }
        }
    }

    findMany(restrictions: Partial<T>): ChiselIterator<T> {
        const i = new Filter(this.inner.columns, restrictions, this.inner);
        return new ChiselIterator(i);
    }

    async findOne(restrictions: Partial<T>): Promise<T | null> {
        const i = new Filter(this.inner.columns, restrictions, this.inner);
        const chiselIterator = new ChiselIterator(i);
        chiselIterator.inner.limit = 1;
        for await (const t of chiselIterator) {
            return t;
        }
        return undefined;
    }

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
        return new ChiselIterator<T & U>(i);
    }

    async forEach(func: (arg: T) => void): Promise<void> {
        for await (const t of this) {
            func(t);
        }
    }

    async toArray(): Promise<Partial<T>[]> {
        const arr = new Array<Partial<T>>();
        for await (const t of this) {
            arr.push(t);
        }
        return arr;
    }

    [Symbol.asyncIterator]() {
        const rid = Deno.core.opSync(
            "chisel_relational_query_create",
            this.inner,
        );

        return {
            async next() {
                const value = await Deno.core.opAsync(
                    "chisel_relational_query_next",
                    rid,
                );
                if (value) {
                    return { value: value, done: false };
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

// We have to pass the columns as a runtime argument since there is no
// way in typescript to reflect on T to get the keys as strings. This
// makes constructing chiselIterators a bit annoying, but that is probably fine
// as we will create the chiselIterator objects, no the chiselstrike users.
export function chiselIterator<T>(name: string, columns: column[]) {
    const b = new BackingStore(columns, name);
    return new ChiselIterator<T>(b);
}
