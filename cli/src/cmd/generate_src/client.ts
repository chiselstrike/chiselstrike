export function createChiselClient(
    serverUrl: string,
    params?: Ωlib.ClientParams,
) {
    return ΩcreateClient(serverUrl, params ?? {});
}
