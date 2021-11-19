export default async function chisel(req) {
    if (req.method == 'PUT') {
        try {
            await Chisel.store('Person', await req.json());
            return Chisel.json("ok");
        } catch (e) {
            return Chisel.json(e, 500);
        }
    }
    return Chisel.json("Only PUT is allowed", 405);
}
