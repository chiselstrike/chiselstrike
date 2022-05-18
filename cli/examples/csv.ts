// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
import { Person } from "../models/person.ts";

export default async function chisel(req: Request) {
    const lines = (await req.text()).split('\n');
    for (const line of lines) {
        const r = line.split(',');
        if (r.length >= 2) {
            const person = new Person();
            person.first_name = r[0];
            person.last_name = r[1];
            person.age = 100;
            person.human = true;
            person.height = 5.0;
            await person.save();
        }
    }
    return new Response('ok');
}
