// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

// The endpoint should evaluate to a promise that resolves to a
// Response.
async function chisel(req) {
    const response = await fetch(req + "/portal/wikipedia.org/assets/img/Wikipedia-logo-v2@2x.png");
    return new Response(response.body, {
        status: 203,
        headers: [
            ["foo", "bar"],
            ["baz", "zed"]
        ]
    });
}
