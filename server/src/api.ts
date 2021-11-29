// In the beginning, we shall implement the following querying logic (with the sole exception of the lambdas,
// which can be replaced by simple Attribute compare logic):
//
// select(Table<T>, Table<T>::Attribute attributes...) -> Table<attributes...>
// filter(Table<T>, fn(T)->bool) -> Table<T>
// sort(Table<T>, fn(T)->Sortable) -> Table<T>
// take(Table<T>, int) -> Table<T>  (takes first n rows)
// join(Table<T>, Table<U>, Table<T>::Attribute, Table<U>::Attribute) -> Table<Composite<T, U>> (Joins tables T and U, based on their columns Table<T>::Attribute and Table<U>::Attribute)
// left_join(Table<T>, Table<U>, Table<T>::Attribute, Table<U>::Attribute) -> Table<Composite<T, Option<U>>>
// right_join(Table<T>, Table<U>, Table<T>::Attribute, Table<U>::Attribute) -> Table<Composite<Option<T>, U>>
// transform(Table<T>, fn(T)->U)->Table<U> (ambitious, maybe later)
//
// Where Table<T>::Attribute represents attribute (field) of type (table) T.

class Base {
    constructor(public columns: string[]) {}
}

// This represents a selection of some columns of a table in a DB.
class BackingStore extends Base {
    // The kind member is use to implement fully covered switch statements.
    readonly kind = "BackingStore";
    constructor(public columns: string[], public name: string) {
        super(columns);
    }
}

// This represents an inner join between two tables.
// FIXME: Add support for ON.
class Join extends Base {
    readonly kind = "Join";
    constructor(
        public columns: string[],
        public left: Inner,
        public right: Inner,
    ) {
        super(columns);
    }
}

// We will add | Filter | Join | ...
type Inner = BackingStore | Join;

function sqlImpl(table: Inner, aliasCount: { v: number }): string {
    switch (table.kind) {
        case "BackingStore":
            return `SELECT ${table.columns} FROM ${table.name}`;
        case "Join": {
            // FIXME: Optimize the case of table.left or table.right being just
            // a BackingStore with all fields. The database probably doesn't
            // care, but will make the logs cleaner.
            const lsql = sqlImpl(table.left, aliasCount);
            const rsql = sqlImpl(table.right, aliasCount);

            const leftAlias = `A${aliasCount.v}`;
            const rightAlias = `A${aliasCount.v + 1}`;
            aliasCount.v += 2;

            const leftSet = new Set(table.left.columns);
            const rightSet = new Set(table.right.columns);
            const joinColumns = [];
            const onColumns = [];
            for (const c of table.columns) {
                if (leftSet.has(c) && rightSet.has(c)) {
                    joinColumns.push(`${leftAlias}.${c}`);
                    onColumns.push(`${leftAlias}.${c} = ${rightAlias}.${c}`);
                } else {
                    joinColumns.push(c);
                }
            }

            const on = onColumns.length === 0
                ? "TRUE"
                : onColumns.join(" AND ");
            // Funny way to write it, but works on PostgreSQL and sqlite.
            const join =
                `(${lsql}) AS ${leftAlias} JOIN (${rsql}) AS ${rightAlias}`;
            return `SELECT ${joinColumns} FROM ${join} ON ${on}`;
        }
    }
}

function sql(table: Inner): string {
    return sqlImpl(table, { v: 0 });
}

function unique<T>(a: T[]) {
    return [...new Set(a)];
}

export class Table<T> {
    constructor(private inner: Inner) {}
    select<C extends (keyof T)[]>(...columns: C): Table<Pick<T, C[number]>> {
        const cs = columns as string[];
        switch (this.inner.kind) {
            case "BackingStore":
                return table(this.inner.name, cs);
            case "Join": {
                const i = new Join(cs, this.inner.left, this.inner.right);
                return new Table(i);
            }
        }
    }

    join<U>(right: Table<U>) {
        const columns = unique(this.inner.columns.concat(right.inner.columns));
        const i = new Join(columns, this.inner, right.inner);
        return new Table<T & U>(i);
    }

    // FIXME: This is not the final interface. Depending on the DB we
    // will to be able to execute all queries with a single SQL
    // statement. We should have a query function on a connection that
    // takes a table and returns an iterator over the rows.
    sql() {
        return sql(this.inner);
    }

    // FIXME: This is not the final API, we should return an iterator,
    // not an array.
    rows() {
        const ret: Record<string, unknown> = {};
        for (const c of this.inner.columns) {
            ret[c] = 42;
        }
        // FIXME: The type assertion is wrong, this is here just to
        // test the types.
        return [ret] as T[];
    }
}

// We have to pass the columns as a runtime argument since there is no
// way in typescript to reflect on T to get the keys as strings. This
// makes constructing tables a bit annoying, but that is probably fine
// as we will create the table objects, no the chiselstrike users.
export function table<T>(name: string, columns: string[]) {
    const b = new BackingStore(columns, name);
    return new Table<T>(b);
}
