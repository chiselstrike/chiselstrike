// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
export default async function chisel(req) {
    let req_json = await req.json();
    let response = "";
    let people = await Chisel.find_all_by("Person", req_json.field_name, req_json.value);
    for await (let person of people) {
        let fields = [person.first_name, person.last_name, person.age, person.human, person.height];
        response += fields.join(" ");
        response += " ";
    }
    response += "\n";
    return new Response(response);
}
