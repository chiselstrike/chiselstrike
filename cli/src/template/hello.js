// Example ChiselStrike endpoint that supports GET and POST.
//
// To access the endpoint, run:
//
// curl localhost:8080/hello

export default async function chisel(req) {
    return Chisel.json("hello, world!")
}