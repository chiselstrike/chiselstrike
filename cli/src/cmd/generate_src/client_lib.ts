export type ClientConfig = {
    /**
     * The base endpoint URL of the ChiselStrike backend.
     *
     * For example, this will be "http://localhost:8080" during local
     * development.
     */
    serverUrl: string;
    /**
     * The version of the ChiselStrike backend to use. This becomes the first
     * path segment in the endpoint URL. If omitted, the default is the version
     * of the backend at the time `chisel generate` was run.
     */
    version?: string;
    /**
     * Additional HTTP headers to provide with each request.
     */
    headers?: Headers | Record<string, string>;
};

export type InternalClientConfig = {
    serverUrl: string;
    version?: string;
    headers?: Headers;
};

export function cliConfigToInternal(
    cliConfig: ClientConfig,
): InternalClientConfig {
    const config: InternalClientConfig = { serverUrl: cliConfig.serverUrl };
    if (cliConfig !== undefined) {
        if (cliConfig.headers !== undefined) {
            config.headers = new Headers(cliConfig.headers);
        }
        if (cliConfig.version !== undefined) {
            config.version = cliConfig.version;
        }
    }
    return config;
}

type InputHeaders = Headers | Record<string, string> | undefined;
function mergeHeaders(
    baseHeaders: Headers | Record<string, string> | undefined,
    otherHeaders: Headers | Record<string, string> | undefined,
): Headers | undefined {
    if (baseHeaders === undefined && otherHeaders === undefined) {
        return undefined;
    }
    const base = new Headers(baseHeaders);
    const other = new Headers(otherHeaders);

    other.forEach((value: string, key: string) => {
        base.set(key, value);
    });
    return base;
}

export function urlJoin(...urlParts: string[]): URL {
    let url = urlParts[0] || "";
    for (let i = 1; i < urlParts.length; i++) {
        const part = urlParts[i];
        if (!url.endsWith("/")) {
            url += "/";
        }
        if (part.startsWith("/")) {
            url += part.slice(1);
        } else {
            url += part;
        }
    }
    return new URL(url);
}

function assertNever(x: never): never {
    return x;
}

async function throwOnError(resp: Response) {
    if (!resp.ok) {
        // TODO: Improve error handling
        throw Error(
            `failed to post an entity. Got error code ${resp.status} (${resp.statusText}) with message: '${await resp
                .text()}'`,
        );
    }
}

async function sendJson(
    url: URL,
    method: string,
    body: unknown,
    cliHeaders?: Headers,
): Promise<Response> {
    const headers = cliHeaders ?? new Headers();
    headers.set("Content-Type", "application/json");

    const resp = await fetch(url, {
        method,
        headers,
        body: JSON.stringify(body),
    });
    await throwOnError(resp);
    return resp;
}

class AccessContext {
    constructor(private context: string) {}

    static fromEntity(entityName: string): AccessContext {
        return new AccessContext(entityName);
    }

    onField(field: string): AccessContext {
        return new AccessContext(this.context + `.${field}`);
    }

    onArray(idx?: number): AccessContext {
        if (idx !== undefined) {
            return new AccessContext(this.context + `[${idx}]`);
        } else {
            return new AccessContext(this.context + `[]`);
        }
    }

    toString(): string {
        return this.context;
    }
}

function entityFromJson<Entity>(
    entityType: reflect.Entity,
    inputValue: Record<string, unknown>,
    entityContext?: AccessContext,
): Entity {
    const entityValue: Record<string, unknown> = {};
    const entityName = entityType.name;
    entityContext = entityContext ?? AccessContext.fromEntity(entityName);

    for (const field of entityType.fields) {
        if (!(field.name in inputValue)) {
            continue;
        }
        const fieldName = field.name;
        const fieldValue = inputValue[fieldName];
        const context = entityContext.onField(fieldName);

        if (fieldValue === null || fieldValue === undefined) {
            if (field.isOptional) {
                entityValue[fieldName] = undefined;
                continue;
            } else {
                throw new Error(
                    `${context} is not optional but undefined/null was received for the field`,
                );
            }
        }

        const err = (typeName: string) => {
            return Error(
                `${context} is of type ${typeName}, but provided value is of type ${typeof fieldValue}`,
            );
        };
        const fieldType = field.type.name;
        if (fieldType === "string" || fieldType === "entityId") {
            if (typeof fieldValue !== "string") {
                throw err("string");
            }
            entityValue[fieldName] = fieldValue;
        } else if (fieldType === "number") {
            if (typeof fieldValue !== "number") {
                throw err("number");
            }
            entityValue[fieldName] = fieldValue;
        } else if (fieldType === "boolean") {
            if (typeof fieldValue !== "boolean") {
                throw err("boolean");
            }
            entityValue[fieldName] = fieldValue;
        } else if (fieldType === "arrayBuffer") {
            entityValue[fieldName] = arrayBufferFromJson(
                context,
                fieldValue,
            );
        } else if (fieldType === "date") {
            entityValue[fieldName] = dateFromJson(
                context,
                fieldValue,
            );
        } else if (fieldType === "array") {
            entityValue[fieldName] = arrayFromJson(
                context,
                field.type.elementType,
                fieldValue,
            );
        } else if (fieldType === "entity") {
            entityValue[fieldName] = nestedEntityFromJson(
                context,
                field.type.entityType,
                fieldValue,
            );
        } else {
            assertNever(fieldType);
            throw new Error(
                `${context} has unexpected type '${fieldType}'`,
            );
        }
    }
    return entityValue as unknown as Entity;
}

function dateFromJson(
    context: AccessContext,
    value: unknown,
): Date {
    if (typeof value === "string" || typeof value === "number") {
        const date = new Date(value);
        if (Number.isNaN(date.valueOf())) {
            throw new Error(
                `failed to convert value to Date for ${context}`,
            );
        }
        return date;
    }
    throw new Error(
        `${context} is of type Date, but received value is not an instance of string nor number`,
    );
}

function arrayBufferFromJson(
    context: AccessContext,
    value: unknown,
): ArrayBuffer {
    if (typeof value === "string") {
        return Uint8Array.from(
            atob(value),
            (c) => c.charCodeAt(0),
        );
    } else {
        throw Error(
            `${context} is of type ArrayBuffer (transported as string), but received value is of type ${typeof value}`,
        );
    }
}

function arrayFromJson(
    context: AccessContext,
    elementType: reflect.Type,
    arrayValue: unknown,
): unknown[] {
    if (Array.isArray(arrayValue)) {
        const elementTypeName = elementType.name;
        const arrayContext = context.onArray();
        switch (elementTypeName) {
            case "string":
            case "number":
            case "boolean":
            case "entityId":
                return arrayValue;
            case "date":
                return arrayValue.map((e) => dateFromJson(arrayContext, e));
            case "arrayBuffer":
                return arrayValue.map((e) =>
                    arrayBufferFromJson(arrayContext, e)
                );
            case "array":
                return arrayValue.map((e) =>
                    arrayFromJson(
                        arrayContext,
                        elementType.elementType,
                        e,
                    )
                );
            case "entity":
                return arrayValue.map((e) =>
                    nestedEntityFromJson(
                        arrayContext,
                        elementType.entityType,
                        e,
                    )
                );
            default:
                assertNever(elementTypeName);
                throw new Error(
                    `${context} has unexpected array element type '${elementTypeName}'`,
                );
        }
    } else {
        throw new Error(
            `${context} is expected to be an Array, but received value is not an instance of Array`,
        );
    }
}

function nestedEntityFromJson(
    context: AccessContext,
    nestedType: reflect.Entity,
    nestedValue: unknown,
): unknown {
    if (typeof nestedValue === "object") {
        type RecordType = Record<string, unknown>;
        return entityFromJson<unknown>(
            nestedType,
            nestedValue as RecordType,
            context,
        );
    } else {
        throw new Error(
            `${context} is Entity, but received value is not an object`,
        );
    }
}

function entityToJson<Entity>(
    entityType: reflect.Entity,
    entityValue: Entity,
    allowMissingId: boolean,
    entityContext?: AccessContext,
): Record<string, unknown> {
    const inputEntity = entityValue as Record<string, unknown>;
    const outputJson: Record<string, unknown> = {};
    const entityName = entityType.name;
    entityContext = entityContext ?? AccessContext.fromEntity(entityName);

    for (const field of entityType.fields) {
        const fieldName = field.name;
        const fieldValue = inputEntity[fieldName];
        const context = entityContext.onField(fieldName);

        if (fieldValue === undefined || fieldValue === null) {
            if (field.isOptional) {
                continue;
            }
            if (fieldName == "id" && allowMissingId) {
                continue;
            }
            throw new Error(
                `${context} is not optional but undefined/null was received for the field`,
            );
        }

        const err = (typeName: string) => {
            return Error(
                `${context} is of type ${typeName}, but provided value is of type ${typeof fieldValue}`,
            );
        };
        const fieldType = field.type.name;
        if (fieldType === "string" || fieldType === "entityId") {
            if (typeof fieldValue !== "string") {
                throw err("string");
            }
            outputJson[fieldName] = fieldValue;
        } else if (fieldType === "number") {
            if (typeof fieldValue !== "number") {
                throw err("number");
            }
            outputJson[fieldName] = fieldValue;
        } else if (fieldType === "boolean") {
            if (typeof fieldValue !== "boolean") {
                throw err("boolean");
            }
            outputJson[fieldName] = fieldValue;
        } else if (fieldType === "arrayBuffer") {
            outputJson[fieldName] = arrayBufferToJson(context, fieldValue);
        } else if (fieldType === "date") {
            outputJson[fieldName] = dateToJson(
                context,
                fieldValue,
            );
        } else if (fieldType === "array") {
            outputJson[fieldName] = arrayToJson(
                context,
                field.type.elementType,
                fieldValue,
                allowMissingId,
            );
        } else if (fieldType === "entity") {
            outputJson[fieldName] = nestedEntityToJson(
                context,
                field.type.entityType,
                fieldValue,
                allowMissingId,
            );
        } else {
            assertNever(fieldType);
            throw new Error(
                `${context} has unexpected type '${fieldType}'`,
            );
        }
    }
    return outputJson;
}

function dateToJson(
    context: AccessContext,
    value: unknown,
): number {
    if (value instanceof Date) {
        return value.getTime();
    }
    throw new Error(
        `${context} is of type Date, but provided value is of different type`,
    );
}

function arrayBufferToJson(_context: AccessContext, value: unknown): string {
    let binary = "";
    const bytes = new Uint8Array(value as ArrayBufferLike);
    const len = bytes.byteLength;
    for (let i = 0; i < len; i++) {
        binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary);
}

function arrayToJson(
    context: AccessContext,
    elementType: reflect.Type,
    arrayValue: unknown,
    allowMissingId: boolean,
): unknown[] {
    if (Array.isArray(arrayValue)) {
        const elementTypeName = elementType.name;
        const arrayContext = context.onArray();
        switch (elementTypeName) {
            case "string":
            case "number":
            case "boolean":
            case "entityId":
                return arrayValue;
            case "arrayBuffer":
                return arrayValue.map(arrayBufferToJson);
            case "date":
                return arrayValue.map((e) => dateToJson(arrayContext, e));
            case "array":
                return arrayValue.map((e) =>
                    arrayToJson(
                        arrayContext,
                        elementType.elementType,
                        e,
                        allowMissingId,
                    )
                );
            case "entity":
                return arrayValue.map((e) => {
                    nestedEntityToJson(
                        arrayContext,
                        elementType.entityType,
                        e,
                        allowMissingId,
                    );
                });
            default:
                assertNever(elementTypeName);
                throw new Error(
                    `${context} has unexpected array element type '${elementTypeName}'`,
                );
        }
    } else {
        throw new Error(
            `expected Array for ${context}, but provided value is not an instance of Array`,
        );
    }
}

function nestedEntityToJson(
    context: AccessContext,
    nestedType: reflect.Entity,
    nestedValue: unknown,
    allowMissingId: boolean,
): Record<string, unknown> {
    if (typeof nestedValue === "object") {
        return entityToJson<unknown>(
            nestedType,
            nestedValue as unknown,
            allowMissingId,
            context,
        );
    } else {
        throw new Error(
            `${context} is of type Entity, but provided value is not an object`,
        );
    }
}

export function makeGetOne<Entity>(
    url: URL,
    entityType: reflect.Entity,
    cliConfig: InternalClientConfig,
): (headers?: Headers | Record<string, string>) => Promise<Entity> {
    return async (headers?: Headers | Record<string, string>) => {
        const resp = await fetch(url, {
            method: "GET",
            headers: mergeHeaders(cliConfig.headers, headers),
        });
        await throwOnError(resp);
        return entityFromJson<Entity>(entityType, await resp.json());
    };
}

export type GetParams<Entity> = {
    pageSize?: number;
    offset?: number;
    filter?: FilterExpr<Entity>;
    headers?: Headers | Record<string, string>;
};

export type GetResponse<Entity> = {
    nextPage?: () => Promise<GetResponse<Entity>>;
    nextPageUrl?: string;
    prevPage?: () => Promise<GetResponse<Entity>>;
    prevPageUrl?: string;
    results: Entity[];
};

export function makeGetMany<Entity>(
    origUrl: URL,
    entityType: reflect.Entity,
    cliConfig: InternalClientConfig,
): (params: GetParams<Entity>) => Promise<GetResponse<Entity>> {
    return async function (
        params: GetParams<Entity>,
    ): Promise<GetResponse<Entity>> {
        // We need to make a copy every time so that the original doesn't get
        // modified when paging.
        const url = new URL(origUrl);
        if (params.pageSize !== undefined) {
            url.searchParams.set("page_size", params.pageSize.toString());
        }
        if (params.offset !== undefined) {
            url.searchParams.set("offset", params.offset.toString());
        }
        if (params.filter !== undefined) {
            const encodedFilter = JSON.stringify(params.filter);
            url.searchParams.set("filter", encodedFilter);
        }
        const headers = mergeHeaders(cliConfig.headers, params.headers);

        async function makeResponse(url: URL): Promise<GetResponse<Entity>> {
            const r = await fetch(url, {
                method: "GET",
                headers,
            });
            await throwOnError(r);

            type PagingResponse = {
                next_page?: string;
                prev_page?: string;
                results: Record<string, unknown>[];
            };
            const resp: PagingResponse = await r.json();

            let nextPage = undefined;
            let prevPage = undefined;
            if (resp.next_page !== undefined) {
                nextPage = () => {
                    const url = new URL(resp.next_page!, cliConfig.serverUrl);
                    return makeResponse(url);
                };
            }
            if (resp.prev_page !== undefined) {
                prevPage = () => {
                    const url = new URL(resp.prev_page!, cliConfig.serverUrl);
                    return makeResponse(url);
                };
            }
            return {
                nextPage,
                nextPageUrl: resp.next_page,
                prevPage,
                prevPageUrl: resp.prev_page,
                results: resp.results.map((e) =>
                    entityFromJson<Entity>(entityType, e)
                ),
            };
        }
        return await makeResponse(url);
    };
}

export function makeGetManyIter<Entity>(
    origUrl: URL,
    entityType: reflect.Entity,
    cliConfig: InternalClientConfig,
): (params?: GetParams<Entity>) => AsyncIterable<Entity> {
    const getPage = makeGetMany<Entity>(
        origUrl,
        entityType,
        cliConfig,
    );
    return function (params?: GetParams<Entity>): AsyncIterable<Entity> {
        return {
            [Symbol.asyncIterator]: async function* () {
                let page = await getPage(params ?? {});
                while (true) {
                    for (const e of page.results) {
                        yield e;
                    }
                    if (page.nextPage !== undefined) {
                        page = await page.nextPage();
                    } else {
                        break;
                    }
                }
            },
        };
    };
}

export type GetAllParams<Entity> = {
    limit?: number;
    offset?: number;
    filter?: FilterExpr<Entity>;
    headers?: Headers | Record<string, string>;
};

export function makeGetAll<Entity>(
    origUrl: URL,
    entityType: reflect.Entity,
    cliConfig: InternalClientConfig,
): (params?: GetAllParams<Entity>) => Promise<Entity[]> {
    const makeIter = makeGetManyIter<Entity>(
        origUrl,
        entityType,
        cliConfig,
    );
    return async function (params?: GetAllParams<Entity>): Promise<Entity[]> {
        let iterParams = {};
        let limit;
        if (params !== undefined) {
            iterParams = {
                offset: params.offset,
                filter: params.filter,
                headers: params.headers,
            };
            limit = params.limit;
        }
        const iter = makeIter(iterParams);
        const arr = [];
        for await (const e of iter) {
            if (limit !== undefined && limit <= arr.length) {
                break;
            }
            if (arr.length === 100000) {
                console.warn(
                    `Retrieving more than 100k elements using getAll endpoint (url '${origUrl}'). For performance reasons, please consider using '.getIter' or stricter FilterExpr object.`,
                );
            }
            arr.push(e);
        }
        return arr;
    };
}

// This magic is necessary to allow passing of nested objects without ID. However, once we allow plain objects
// we will have to generate the ID-less entities explictly because otherwise we might accidentaly
// remove 'id' fields from plain objects.
type OmitDistributive<T, K extends PropertyKey> = T extends
    Record<string, unknown> ? OmitRecursively<T, K> : T;
type OmitRecursively<T extends Record<string, unknown>, K extends PropertyKey> =
    Omit<
        { [P in keyof T]: OmitDistributive<T[P], K> },
        K
    >;

export function makePostOne<Entity extends Record<string, unknown>>(
    url: URL,
    entityType: reflect.Entity,
    cliConfig: InternalClientConfig,
): (
    entity: OmitRecursively<Entity, "id">,
    headers?: Headers | Record<string, string>,
) => Promise<Entity> {
    return async (
        entity: OmitRecursively<Entity, "id">,
        headers?: Headers | Record<string, string>,
    ) => {
        const entityJson = entityToJson(entityType, entity, true);
        const resp = await sendJson(
            url,
            "POST",
            entityJson,
            mergeHeaders(cliConfig.headers, headers),
        );
        await throwOnError(resp);
        return entityFromJson<Entity>(entityType, await resp.json());
    };
}

export function makePutOne<Entity extends Record<string, unknown>>(
    url: URL,
    entityType: reflect.Entity,
    cliConfig: InternalClientConfig,
): (
    entity: OmitRecursively<Entity, "id">,
    headers?: Headers | Record<string, string>,
) => Promise<Entity> {
    return async (
        entity: OmitRecursively<Entity, "id">,
        headers?: Headers | Record<string, string>,
    ) => {
        const entityJson = entityToJson(entityType, entity, false);
        const resp = await sendJson(
            url,
            "PUT",
            entityJson,
            mergeHeaders(cliConfig.headers, headers),
        );
        await throwOnError(resp);
        return entityFromJson<Entity>(entityType, await resp.json());
    };
}

export function makePatchOne<Entity>(
    url: URL,
    entityType: reflect.Entity,
    cliConfig: InternalClientConfig,
): (
    entity: Partial<Entity>,
    headers?: Headers | Record<string, string>,
) => Promise<Entity> {
    return async (
        entity: Partial<Entity>,
        headers?: Headers | Record<string, string>,
    ) => {
        const entityJson = entityToJson(entityType, entity, false);
        const resp = await sendJson(
            url,
            "PATCH",
            entityJson,
            mergeHeaders(cliConfig.headers, headers),
        );
        await throwOnError(resp);
        return entityFromJson<Entity>(entityType, await resp.json());
    };
}

export function makeDeleteOne(
    url: URL,
    cliConfig: InternalClientConfig,
): (headers?: Headers | Record<string, string>) => Promise<void> {
    return async (headers?: Headers | Record<string, string>) => {
        const resp = await fetch(url, {
            method: "DELETE",
            headers: mergeHeaders(cliConfig.headers, headers),
        });
        await throwOnError(resp);
    };
}

export function makeDeleteMany<Entity>(
    url: URL,
    cliConfig: InternalClientConfig,
): (
    filter: FilterExpr<Entity>,
    headers?: Headers | Record<string, string>,
) => Promise<void> {
    return async (
        filter: FilterExpr<Entity>,
        headers?: Headers | Record<string, string>,
    ) => {
        url.searchParams.set("filter", JSON.stringify(filter));
        const resp = await fetch(url, {
            method: "DELETE",
            headers: mergeHeaders(cliConfig.headers, headers),
        });
        await throwOnError(resp);
    };
}
