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

async function throwOnError(resp: Response) {
    if (!resp.ok) {
        // TODO: Improve error handling
        throw Error(
            `failed to post an entity. Got error code ${resp.status} (${resp.statusText}) with message: '${await resp
                .text}'`,
        );
    }
}

async function sendJson(
    url: URL,
    method: string,
    body: unknown,
): Promise<Response> {
    const resp = await fetch(url, {
        method,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
    });
    await throwOnError(resp);
    return resp;
}

function jsonToEntity<Entity>(
    entityType: reflect.Entity,
    inputValue: Record<string, unknown>,
): Entity {
    const entityValue: Record<string, unknown> = {};
    const entityName = entityType.name;
    for (const field of entityType.fields) {
        if (!(field.name in inputValue)) {
            continue;
        }
        const fieldName = field.name;
        const fieldValue = inputValue[fieldName];

        if (fieldValue === null || fieldValue === undefined) {
            if (field.isOptional) {
                entityValue[fieldName] = undefined;
                continue;
            } else {
                throw new Error(
                    `field ${fieldName} of entity ${entityName} is not optional but undefined/null was received for the field`,
                );
            }
        }

        const err = (typeName: string) => {
            return Error(
                `field ${field.name} of entity ${entityName} is ${typeName}, but provided value is of type ${typeof fieldValue}`,
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
            entityValue[fieldName] = convertArrayBuffer(
                entityName,
                fieldName,
                fieldValue,
            );
        } else if (fieldType === "date") {
            entityValue[fieldName] = convertDate(
                entityName,
                fieldName,
                fieldValue,
            );
        } else if (fieldType === "array") {
            entityValue[fieldName] = convertArray(
                entityName,
                fieldName,
                field.type.elementType,
                fieldValue,
            );
        } else if (fieldType === "entity") {
            entityValue[fieldName] = convertEntity(
                entityName,
                fieldName,
                field.type.entityType,
                fieldValue,
            );
        } else {
            assertNever(fieldType);
            throw new Error(
                `field '${fieldName}' of entity '${entityName}' has unexpected type '${fieldType}'`,
            );
        }
    }
    return entityValue as unknown as Entity;
}

function convertDate(
    entityName: string,
    fieldName: string,
    value: unknown,
): Date {
    if (typeof value === "string" || typeof value === "number") {
        const date = new Date(value);
        if (Number.isNaN(date.valueOf())) {
            throw new Error(
                `failed to convert value to Date for field ${fieldName}`,
            );
        }
        return date;
    }
    throw new Error(
        `field ${fieldName} of entity ${entityName} is Date, but received value is not an instance of string nor number`,
    );
}

function convertArrayBuffer(
    entityName: string,
    fieldName: string,
    value: unknown,
): ArrayBuffer {
    if (typeof value === "string") {
        return Uint8Array.from(
            atob(value),
            (c) => c.charCodeAt(0),
        );
    } else {
        throw Error(
            `field ${fieldName} of entity ${entityName} is ArrayBuffer (transported as string), but received value is of type ${typeof value}`,
        );
    }
}

function convertArray(
    entityName: string,
    fieldName: string,
    elementType: reflect.Type,
    arrayValue: unknown,
): unknown[] {
    if (Array.isArray(arrayValue)) {
        const elementTypeName = elementType.name;
        switch (elementTypeName) {
            case "string":
            case "number":
            case "boolean":
            case "entityId":
                return arrayValue;
            case "date":
                return arrayValue.map((e) =>
                    convertDate(entityName, fieldName, e)
                );
            case "arrayBuffer":
                return arrayValue.map((e) =>
                    convertArrayBuffer(entityName, fieldName, e)
                );
            case "array":
                return arrayValue.map((e) =>
                    convertArray(
                        entityName,
                        fieldName,
                        elementType.elementType,
                        e,
                    )
                );
            case "entity":
                return arrayValue.map((e) =>
                    convertEntity(
                        entityName,
                        fieldName,
                        elementType.entityType,
                        e,
                    )
                );
            default:
                assertNever(elementTypeName);
                throw new Error(
                    `field '${fieldName}' of entity '${entityName}' has unexpected array element type '${elementTypeName}'`,
                );
        }
    } else {
        throw new Error(
            `expected Array, but received value is not an instance of Array`,
        );
    }
}

function convertEntity(
    entityName: string,
    fieldName: string,
    entityType: reflect.Entity,
    entityValue: unknown,
): unknown {
    if (typeof entityValue === "object") {
        type RecordType = Record<string, unknown>;
        return jsonToEntity<unknown>(
            entityType,
            entityValue as RecordType,
        );
    } else {
        throw new Error(
            `field ${fieldName} of entity ${entityName} is Entity, but received value is not an object`,
        );
    }
}

function assertNever(x: never): never {
    return x;
}

export function makeGetOne<Entity>(
    url: URL,
    entityType: reflect.Entity,
): () => Promise<Entity> {
    return async () => {
        const resp = await fetch(url, { method: "GET" });
        await throwOnError(resp);
        return jsonToEntity<Entity>(entityType, await resp.json());
    };
}

export type GetParams<Entity> = {
    pageSize?: number;
    offset?: number;
    filter?: FilterExpr<Entity>;
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
    serverUrl: string,
    entityType: reflect.Entity,
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

        async function makeResponse(url: URL): Promise<GetResponse<Entity>> {
            const r = await fetch(url, { method: "GET" });
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
                    return makeResponse(new URL(resp.next_page!, serverUrl));
                };
            }
            if (resp.prev_page !== undefined) {
                prevPage = () => {
                    return makeResponse(new URL(resp.prev_page!, serverUrl));
                };
            }
            return {
                nextPage,
                nextPageUrl: resp.next_page,
                prevPage,
                prevPageUrl: resp.prev_page,
                results: resp.results.map((e) =>
                    jsonToEntity<Entity>(entityType, e)
                ),
            };
        }
        return await makeResponse(url);
    };
}

export function makeGetManyIter<Entity>(
    origUrl: URL,
    serverUrl: string,
    entityType: reflect.Entity,
): (params?: GetParams<Entity>) => AsyncIterable<Entity> {
    const getPage = makeGetMany<Entity>(origUrl, serverUrl, entityType);
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
};

export function makeGetAll<Entity>(
    origUrl: URL,
    serverUrl: string,
    entityType: reflect.Entity,
): (params?: GetAllParams<Entity>) => Promise<Entity[]> {
    const makeIter = makeGetManyIter<Entity>(origUrl, serverUrl, entityType);
    return async function (params?: GetAllParams<Entity>): Promise<Entity[]> {
        let iterParams = {};
        let limit;
        if (params !== undefined) {
            iterParams = {
                offset: params.offset,
                filter: params.filter,
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

export function makePostOne<Entity>(
    url: URL,
    entityType: reflect.Entity,
): (entity: Omit<Entity, "id">) => Promise<Entity> {
    return async (entity: Omit<Entity, "id">) => {
        // TODO: We should probably do the inverse of jsonToEntity.
        const resp = await sendJson(url, "POST", entity);
        await throwOnError(resp);
        return jsonToEntity<Entity>(entityType, await resp.json());
    };
}

export function makePutOne<Entity>(
    url: URL,
    entityType: reflect.Entity,
): (entity: Entity) => Promise<Entity> {
    return async (entity: Entity) => {
        const resp = await sendJson(url, "PUT", entity);
        await throwOnError(resp);
        return jsonToEntity<Entity>(entityType, await resp.json());
    };
}

export function makePatchOne<Entity>(
    url: URL,
    entityType: reflect.Entity,
): (entity: Partial<Entity>) => Promise<Entity> {
    return async (entity: Partial<Entity>) => {
        const resp = await sendJson(url, "PATCH", entity);
        await throwOnError(resp);
        return jsonToEntity<Entity>(entityType, await resp.json());
    };
}

export function makeDeleteOne(url: URL): () => Promise<void> {
    return async () => {
        const resp = await fetch(url, { method: "DELETE" });
        await throwOnError(resp);
    };
}

export function makeDeleteMany<Entity>(
    url: URL,
): (filter: FilterExpr<Entity>) => Promise<void> {
    return async (filter: FilterExpr<Entity>) => {
        url.searchParams.set("filter", JSON.stringify(filter));
        const resp = await fetch(url, { method: "DELETE" });
        await throwOnError(resp);
    };
}
