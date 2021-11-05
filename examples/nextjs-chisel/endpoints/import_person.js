function makeResponse(status, msg) {
    let blob = new Blob([JSON.stringify(msg, null, 2)], {type : 'application/json'});
    let init = { "status" : status , "message" : blob };
    return new Response(blob, init);
}

export default async function chisel(req) {
    if (req.method == 'PUT') {
        try {
            await Chisel.store('Person', await req.json());
            return makeResponse(200, "ok");
        } catch (e) {
            return makeResponse(500, e);
        }
    }
    return makeResponse(405, "Only PUT is allowed");
}