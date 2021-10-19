// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

// The endpoint consists of a function that evaluates to a promise
// that resolves to a Response.

function replacer(key, value) {
    const ret = {};
    if (key == "") {
        for (const k in value) {
            ret[k] = value[k];
        }
        return ret;
    }
    if (key == "headers") {
        for (const header of value) {
            ret[header[0]] = header[1];
        }
        return ret;
    }
    return value;
}

async function chisel(req) {
    const body = JSON.stringify(req, replacer, 4);
    return new Response(body + "\n", {
        status: 203,
        headers: [
            ["foo", "bar"],
            ["baz", "zed"]
        ]
    });
}
