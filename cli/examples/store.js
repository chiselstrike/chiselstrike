// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
export default async function chisel(req) {
    if (req.method == 'POST') {
        const payload = await req.json();

        const person = Person.build(payload);
        await person.save();
        return new Response('ok');
    }
    return new Response('ignored');
}
