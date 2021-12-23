export default async function chisel(req) {
    if (req.method == 'GET') {
        try {
            let resp_json = [];
            for await (let p of Chisel.Person) {
                resp_json.push(p);
            }
            return Chisel.json(resp_json);
        } catch (e) {
            return Chisel.json(e, 500);
        }
    }
    return Chisel.json("Only GET is allowed", 405);
}
