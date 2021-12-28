// In the beginning, we shall implement the following querying logic (with the sole exception of the lambdas,
// which can be replaced by simple Attribute compare logic):
//
// select(Table<T>, Table<T>::Attribute attributes...) -> Table<attributes...>
// findMany(Table<T>, fn(T)->bool) -> Table<T>
// sort(Table<T>, fn(T)->Sortable) -> Table<T>
// take(Table<T>, int) -> Table<T>  (takes first n rows)
// join(Table<T>, Table<U>, Table<T>::Attribute, Table<U>::Attribute) -> Table<Composite<T, U>> (Joins tables T and U, based on their columns Table<T>::Attribute and Table<U>::Attribute)
// left_join(Table<T>, Table<U>, Table<T>::Attribute, Table<U>::Attribute) -> Table<Composite<T, Option<U>>>
// right_join(Table<T>, Table<U>, Table<T>::Attribute, Table<U>::Attribute) -> Table<Composite<Option<T>, U>>
// transform(Table<T>, fn(T)->U)->Table<U> (ambitious, maybe later)
//
// Where Table<T>::Attribute represents attribute (field) of type (table) T.

type column = [string, string]; // name and type

class Base {
    limit?: bigint;
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

// This represents an inner join between two tables.
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

export class Table<T> {
    constructor(private inner: Inner) {}
    select(...columns: (keyof T)[]): Table<Pick<T, (keyof T)>> {
        const names = columns as string[];
        const cs = this.inner.columns.filter((c) => names.includes(c[0]));
        switch (this.inner.kind) {
            case "BackingStore":
                return table(this.inner.name, cs);
            case "Join": {
                const i = new Join(cs, this.inner.left, this.inner.right);
                return new Table(i);
            }
            case "Filter": {
                const i = new Filter(
                    cs,
                    this.inner.restrictions,
                    this.inner.inner,
                );
                return new Table(i);
            }
        }
    }

    take(limit: bigint): Table<T> {
        if (this.inner.limit == null) {
            this.inner.limit = limit;
        } else {
            this.inner.limit = Math.min(limit, this.inner.limit);
        }
        return this;
    }

    findMany(restrictions: Partial<T>): Table<T> {
        const i = new Filter(this.inner.columns, restrictions, this.inner);
        return new Table(i);
    }

    async findOne(restrictions: Partial<T>): T | null {
        const i = new Filter(this.inner.columns, restrictions, this.inner);
        i.limit = 1;
        const table = new Table(i);
        for await (const t of table) {
            return t;
        }
        return undefined;
    }

    join<U>(right: Table<U>) {
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
        return new Table<T & U>(i);
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
                return { done: true };
            },
        };
    }
}

// We have to pass the columns as a runtime argument since there is no
// way in typescript to reflect on T to get the keys as strings. This
// makes constructing tables a bit annoying, but that is probably fine
// as we will create the table objects, no the chiselstrike users.
export function table<T>(name: string, columns: column[]) {
    const b = new BackingStore(columns, name);
    return new Table<T>(b);
}
