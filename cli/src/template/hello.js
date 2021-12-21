// Example ChiselStrike endpoint.
//
// To access the endpoint, run:
//
// curl -d '{"hello": "world"}' localhost:8080/dev/hello

export default async function (req) {
    const json = await req.json();
    return Chisel.json(json);
}
