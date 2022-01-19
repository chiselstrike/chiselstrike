// Example ChiselStrike endpoint.
//
// To access the endpoint, run:
//
// curl -d '{"hello": "world"}' localhost:8080/dev/hello

import { responseFromJson } from "@chiselstrike/api";

export default async function (req: Request): Promise<Response> {
    const json = await req.json();
    return responseFromJson(json);
}
