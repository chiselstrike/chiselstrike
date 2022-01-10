// SPDX-FileCopyrightText: © 2021-2022 ChiselStrike <info@chiselstrike.com>
export default async function chisel(req) {
    if (req.method == 'POST') {
        const payload = await req.json();
        // FIXME: provide a better way to generate an object from Json
        const person = new Person();
        if ( "first_name" in payload ) {
            person.first_name = payload["first_name"]
        }

        if ( "last_name" in payload ) {
            person.last_name = payload["last_name"]
        }

        if ( "age" in payload ) {
            person.age = payload["age"]
        }

        if ( "human" in payload ) {
            person.human = payload["human"]
        }

        if ( "height" in payload ) {
            person.height = payload["height"]
        }

        await person.save();
        return new Response('ok');
    }
    return new Response('ignored');
}
