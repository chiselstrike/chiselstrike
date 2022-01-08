// Example ChiselStrike endpoint.
//
// To access the endpoint, run:
//
// curl -d '{"hello": "world"}' localhost:8080/dev/hello

import { Chisel } from "@chiselstrike/chiselstrike";

export default async function (req: Request): Promise<Response> {
    const json = await req.json();
    return Chisel.json(json);
}
