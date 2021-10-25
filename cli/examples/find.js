// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
async function chisel(req) {
    let response = "";
    let people = await Chisel.find_all("Person");
    for await (let person of people) {
        response += person.first_name;
        response += " ";
    }
    return new Response(response);
}
