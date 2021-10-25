Chisel = {}
Chisel.buildReadableStreamForBody = function(rid) {
    return new ReadableStream({
        async pull(controller) {
            const chunk = await Deno.core.opAsync("deno_read_body", rid);
            if (chunk) {
                controller.enqueue(chunk);
            } else {
                controller.close();
            }
        }
    });
}
