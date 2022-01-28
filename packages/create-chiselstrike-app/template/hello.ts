// Example ChiselStrike endpoint.
//
// To access the endpoint, run:
//
// curl -d '{"hello": "world"}' localhost:8080/dev/hello

export default async function (req: Request): Promise<Response> {
    const response = await req.text() || "hello world";
    return new Response(response);
}
