export type PrimitiveValue = string | number | boolean | null | undefined;
export type EntityValue = PrimitiveValue | { [key: string]: EntityValue };

export type ComparisonOperator =
    | "$eq"
    | "$gt"
    | "$gte"
    | "$lt"
    | "$lte"
    | "$ne";

export type FieldFilter = PrimitiveValue | {
    ComparisonOperator: PrimitiveValue;
} | {
    [key: string]: FieldFilter;
};

export type FilterExpr<T> = {
    "$and"?: FilterExpr<T>[];
    "$or"?: FilterExpr<T>[];
    "$not"?: FilterExpr<T>;
} & { [key in keyof Partial<T>]: FieldFilter };

export function evalFilter<T>(
    filterExpr: FilterExpr<T>,
    v: Record<string, unknown>,
): boolean {
    return Object.entries(filterExpr).every(([key, filter]) => {
        if (key === "$and") {
            const operands = filter as FilterExpr<T>[];
            return operands.every((innerFilter: FilterExpr<T>) =>
                evalFilter(innerFilter, v)
            );
        } else if (key === "$or") {
            const operands = filter as FilterExpr<T>[];
            return operands.some((innerFilter: FilterExpr<T>) =>
                evalFilter(innerFilter, v)
            );
        } else if (key === "$not") {
            return !evalFilter(filter as FilterExpr<T>, v);
        } else {
            return evalFieldFilter(
                filter as FieldFilter,
                v[key] as EntityValue,
            );
        }
    });
}

function evalFieldFilter(filter: FieldFilter, v: EntityValue): boolean {
    const valueIsObj = typeof v === "object" && v !== null;
    if (typeof filter === "object" && filter !== null) {
        return Object.entries(filter).every(([key, filterValue]) => {
            if (valueIsObj) {
                if (key in v) {
                    return evalFieldFilter(filterValue, v[key]);
                } else {
                    throw Error(`key '${key} not contained in entity value`);
                }
            } else {
                return evalOperator(
                    key as ComparisonOperator,
                    filterValue as PrimitiveValue,
                    v as PrimitiveValue,
                );
            }
        });
    } else {
        if (valueIsObj) {
            throw Error(
                `failed to filter with filter value '${
                    JSON.stringify(filter)
                }'`,
            );
        }
        return v === filter;
    }
}

function evalOperator(
    cmpOp: ComparisonOperator,
    filterValue: PrimitiveValue,
    v: PrimitiveValue,
) {
    if (cmpOp === "$eq") {
        return v === filterValue;
    } else if (cmpOp === "$ne") {
        return v !== filterValue;
    }
    if (v === null || v === undefined) {
        throw Error(
            `can't apply comparison operator '${
                JSON.stringify(cmpOp)
            }' to a value '${v}'`,
        );
    } else if (filterValue === null || filterValue === undefined) {
        throw Error(
            `can't apply comparison operator '${
                JSON.stringify(cmpOp)
            }' to a filter value '${filterValue}'`,
        );
    }
    if (cmpOp === "$gt") {
        return v > filterValue;
    } else if (cmpOp === "$gte") {
        return v >= filterValue;
    } else if (cmpOp === "$lt") {
        return v < filterValue;
    } else if (cmpOp === "$lte") {
        return v <= filterValue;
    } else {
        throw Error(
            `trying to filter with an unexpected operator '${
                JSON.stringify(cmpOp)
            }'`,
        );
    }
}
