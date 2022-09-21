// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
import { RouteMap } from "@chiselstrike/api";
import { Person } from "../models/person.ts";

async function handlePost(req: Request): Promise<Response> {
    const payload = await req.json();

    const person = Person.build(payload);
    await person.save();
    return new Response('ok');
}

function handleGet(req: Request): Response {
    return new Response("ignored");
}

export default new RouteMap()
    .post("/", handlePost)
    .get("/", handleGet);
