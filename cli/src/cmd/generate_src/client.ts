export type ClientParams = Ωlib.ClientParams;
export function createChiselClient(
    serverUrl: string,
    params?: ClientParams,
) {
    return ΩcreateClient(serverUrl, params ?? {});
}
