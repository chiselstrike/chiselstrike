export default async function chisel(_req) {
    const { status, body, headers } = await Deno.core.opAsync(
        "op_chisel_auth_user",
        Chisel.requestContext.userId,
    );
    return new Response(body, { status, headers });
}
