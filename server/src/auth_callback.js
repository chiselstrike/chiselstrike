export default async function chisel(req) {
    const { status, body, headers } = await Deno.core.opAsync(
        "op_chisel_auth_callback",
        req.url,
    );
    return new Response(body, { status, headers });
}
