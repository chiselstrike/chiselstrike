// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
import { Person } from "../models/person.ts";

export default async function chisel(req: Request) {
    let response = "";
    for await (let person of Person.cursor()) {
        let fields = [person.first_name, person.last_name, person.age, person.human, person.height];
        response += fields.join(" ");
        response += " ";
    }
    return new Response(response);
}
