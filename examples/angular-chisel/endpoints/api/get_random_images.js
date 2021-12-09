function makeResponse(status, msg) {
    var blob = new Blob([JSON.stringify(msg, null, 2)], {type : 'application/json'});
    var init = { "status" : status , "message" : blob };
    return new Response(blob, init);
}

export default async function chisel(req) {
    if (req.method == 'GET') {
        try {
            let images = await Chisel.collections.Image.rows();
            let resp_json = [];
            for await (let img of images) {
                resp_json.push({
                    filename: img.filename,
                    data: img.data,
                    width: img.width,
                    height: img.height
                });
            }
            resp_json = resp_json.sort(() => .5 - Math.random()).slice(0,5)
            return makeResponse(200, resp_json);
        } catch (e) {
            return makeResponse(500, e);
        }
    }
    return makeResponse(405, "Only GET is allowed");
}
