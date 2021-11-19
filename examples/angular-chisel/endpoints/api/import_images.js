async function fileToBase64(file) {
    const reader = new FileReader();
    return new Promise((resolve) => {
        reader.onloadend = () => resolve(reader.result);
        reader.readAsDataURL(file);
    });
}
  
// async function parseImage(file) {
//     const img_base64 = await fileToBase64(file);
//     return new Promise((resolve) => {
//         let img = new Image();
//         img.onload = () => {
//             resolve(
//                 height: img.height,
//                 width: img.width,
//                 data: img.src
//             );
//         };
//         img.src = img_base64;
//     });
// }

function makeResponse(status, msg) {
    let blob = new Blob([JSON.stringify(msg, null, 2)], {type : 'application/json'});
    let init = { "status" : status , "message" : blob };
    return new Response(blob, init);
}

export default async function chisel(req) {
    if (req.method == 'PUT') {
        try {
            for (let data of await req.formData()) {
                const filename = data[0];
                const file = data[1];
                await Chisel.store('Image', {
                    name: filename,
                    data: await fileToBase64(file),
                    width: 0,
                    height: 0
                });
            }
            return makeResponse(200, "ok");
        } catch (e) {
            return makeResponse(500, e);
        }
    }
    return makeResponse(405, "Only PUT is allowed");
}