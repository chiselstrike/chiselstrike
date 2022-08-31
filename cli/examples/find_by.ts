// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
import { RouteMap } from "@chiselstrike/api";
import { Person } from "../models/person.ts";

async function handlePost(req: Request): Promise<Response> {
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

export default new RouteMap()
    .post("/", handlePost);
