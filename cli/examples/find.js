// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
export default async function chisel(req) {
    let response = "";
    for await (let person of Chisel.Person) {
        let fields = [person.first_name, person.last_name, person.age, person.human, person.height];
        response += fields.join(" ");
        response += " ";
    }
    response += "\n"
    return new Response(response);
}
