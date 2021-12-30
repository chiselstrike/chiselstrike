// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
export default async function chisel(req) {
    const lines = (await req.text()).split('\n');
    for (const line of lines) {
        const r = line.split(',');
        if (r.length >= 2) {
            await Chisel.save('Person', {"first_name": r[0], "last_name": r[1], "age": 100, "human": true, "height": 5.0});
        }
    }
    return new Response('ok\n');
}
