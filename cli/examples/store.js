// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
export default async function chisel(req) {
    if (req.method == 'POST') {
        const payload = await req.json();
        await Chisel.save('Person', payload);
        return new Response('ok\n');
    }
    return new Response('ignored\n');
}
