// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
import { Person } from "../models/person.ts";

export default async function chisel(req: Request) {
    let req_json = await req.json();
    let response = "";
    let filter_obj = {[req_json.field_name]: req_json.value};
    for await (let person of Person.cursor().filter(filter_obj)) {
        let fields = [person.first_name, person.last_name, person.age, person.human, person.height];
        response += fields.join(" ");
        response += " ";
    }
    return new Response(response);
}
