export default async function chisel(req) {
    let response = "";
    let posts = await Chisel.find_all("Post");
    for await (let post of posts) {
        response += post.title;
        response += " ";
    }
    return new Response(JSON.stringify({"response": response}), {
        headers: [
            ["content-type", "application/json"]
        ]
    });
}
