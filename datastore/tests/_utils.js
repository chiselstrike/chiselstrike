function assertFail(message) {
    if (typeof message === "function") {
        message = message();
    }
    throw new Error(message);
}

function assert(value, message) {
    if (!value) {
        assertFail(message ?? "value is not true");
    }
}

function assertEq(left, right, message) {
    if (left !== right) {
        assertFail(message ?? `${left} !== ${right}`);
    }
}

function assertJsonEq(left, right, message) {
    if (!jsonEq(left, right)) {
        assertFail(message ?? `${JSON.stringify(left)} does not equal ${JSON.stringify(right)}`);
    }
}

function jsonEq(left, right) {
    if (typeof left !== typeof right) {
        return false;
    } else if (left === right) {
        return true;
    } else if (typeof left === "object") {
        for (const key in left) {
            if (!(key in right) || !jsonEq(left[key], right[key])) {
                return false;
            }
        }
        for (const key in right) {
            if (!(key in left)) {
                return false;
            }
        }
        return true;
    } else if (Array.isArray(left) && Array.isArray(right)) {
        if (left.length !== right.length) {
            return false;
        }
        for (let i = 0; i < left.length; ++i) {
            if (!jsonEq(left[i], right[i])) {
                return false;
            }
        }
        return true;
    } else {
        return false;
    }
}

function assertThrows(pattern, callback) {
    function checkErr(e) {
        const errorStr = ""+e;
        if (!errorStr.includes(pattern)) {
            throw new Error(`expected an error matching ${JSON.stringify(pattern)}, ` +
                `got ${JSON.stringify(errorStr)}`)
        }
    }

    let ret;
    try {
        ret = callback();
    } catch (e) {
        checkErr(e);
    }

    if (ret.then) {
        const onFulfilled = () => { throw new Error("expected the promise to throw an error") };
        const onRejected = (e) => checkErr(e);
        return ret.then(onFulfilled, onRejected);
    }

    throw new Error("expected the callback to throw an error");
}

function println(val) {
    Deno.core.opSync("op_test_println", val);
}
