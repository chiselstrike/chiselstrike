// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
export default async function chisel(req) {
    let response = "";
    let people = await Chisel.collections.Person.rows();
    for await (let person of people) {
        let fields = [person.first_name, person.last_name, person.age, person.human, person.height];
        response += fields.join(" ");
        response += " ";
    }
    return new Response(response);
}
