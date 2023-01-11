export function assert(expr: unknown, msg = "") {
    if (!expr) {
        throw new Error(msg);
    }
}

export function assertEquals<T>(actual: T, expected: T, msg?: string) {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        throw new Error(msg ?? `actual (${actual}) != expected (${expected})`);
    }
}
