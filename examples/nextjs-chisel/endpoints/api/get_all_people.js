export default async function chisel(req) {
    if (req.method == 'GET') {
        try {
            let images = await Chisel.find_all("Person");
            let resp_json = [];
            for await (let img of images) {
                resp_json.push(img);
            }
            return Chisel.json(resp_json);
        } catch (e) {
            return Chisel.json(e, 500);
        }
    }
    return Chisel.json("Only GET is allowed", 405);
}