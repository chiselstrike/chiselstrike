// Example ChiselStrike endpoint.
//
// To access the endpoint, run:
//
// curl -d '{"hello": "world"}' localhost:8080/dev/hello
import { ChiselRequest } from "@chiselstrike/api";

export default async function (req: ChiselRequest): Promise<string> {
    return await req.text() || "hello world";
}
