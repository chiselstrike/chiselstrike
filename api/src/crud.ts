import { opAsync, responseFromJson } from './utils.ts';
import { ChiselEntity, requestContext } from './datastore.ts';
import { ChiselRequest } from './request.ts';
import { RouteMap } from './routing.ts';

type ChiselEntityClass<T extends ChiselEntity> = {
    new (): T;
    findOne: (_: { id: string }) => Promise<T | undefined>;
    findMany: (_: Partial<T>) => Promise<T[]>;
    build: (...properties: Record<string, unknown>[]) => T;
    delete: (_: { id: string }) => Promise<void>;
};

/** Creates a Response object from response body and status. */
export type CRUDCreateResponse = (
    body: unknown,
    status: number,
) => Promise<Response> | Response;


/**
 * Generates a route map to handle REST methods GET/PUT/POST/DELETE for this entity.
 * @example
 * Put this in the file 'endpoints/comments.ts':
 * ```typescript
 * import { Comment } from "../models/comment";
 * export default crud(Comment, ":id");
 * ```
 * This results in a /comments endpoint that correctly handles all REST methods over Comment.
 * @param entity Entity type
 * @param config Configure the CRUD behavior:
 *  - `createResponse`: function to create response from entity or an array of entities. Defaults to
 *    `responseFromJson()`.
 *  - `getAll`: should we generate `GET /` route that returns all entities according to filters in the URL
 *    query parameters? Defaults to true.
 *  - `getOne`: should we generate `GET /:id` route that returns one entity by id? Defaults to true.
 *  - `post`: should we generate `POST /` route that creates a new entity? Defaults to true.
 *  - `put`: should we generate `PUT /:id` route that creates or updates an entity? Defaults to true.
 *  - `deleteAll`: should we generate `DELETE /` route that deletes all entities according to filters in the
 *    URL query parameters? Defaults to true.
 *  - `deleteOne`: should we generate `DELETE /:id` route that deletes one entity by id? Defaults to true.
 * @returns A route map suitable as a default export in a route file.
 */
export function crud<
    T extends ChiselEntity,
    E extends ChiselEntityClass<T>,
>(
    entity: E,
    config?: {
        createResponse?: CRUDCreateResponse,
        getAll?: boolean,
        getOne?: boolean,
        post?: boolean,
        put?: boolean,
        deleteAll?: boolean,
        deleteOne?: boolean,
    },
): RouteMap {
    const createResponse = config?.createResponse ?? responseFromJson;
    const routeMap = new RouteMap();

    // Returns all entities matching the filter in the `filter` URL parameter.
    async function getAll(req: ChiselRequest): Promise<Response> {
        return createResponse(await fetchEntitiesCrud(entity, req.path, Array.from(req.query)), 200);
    }
    if (config?.getAll ?? true)
        routeMap.get('/', getAll);

    // Returns a specific entity matching :id
    async function getOne(req: ChiselRequest): Promise<Response> {
        const id = req.params.get('id');
        const u = await entity.findOne({ id });
        if (u !== undefined) {
            return createResponse(u, 200);
        } else {
            return createResponse("Not found", 404);
        }
    }
    if (config?.getOne ?? true)
        routeMap.get('/:id', getOne);

    // Creates and returns a new entity from the `req` payload. Ignores the payload's id property and assigns a fresh one.
    async function post(req: ChiselRequest): Promise<Response> {
        const u = entity.build(await req.json());
        u.id = undefined;
        await u.save();
        return createResponse(u, 200);
    }
    if (config?.post ?? true)
        routeMap.post('/', post);

    // Updates and returns the entity matching :id from the `req` payload.
    async function put(req: ChiselRequest): Promise<Response> {
        const u = entity.build(await req.json());
        u.id = req.params.get('id');
        await u.save();
        return createResponse(u, 200);
    }
    if (config?.put ?? true)
        routeMap.put('/:id', put);

    // Deletes all entities matching the filter in the `filter` URL parameter.
    async function deleteAll(req: ChiselRequest): Promise<Response> {
        await deleteEntitiesCrud(entity, Array.from(req.query));
        return createResponse(`Deleted entities matching ${new URL(req.url).search}`, 200);
    }
    if (config?.deleteAll ?? true)
        routeMap.delete('/', deleteAll);

    // Deletes the entity matching :id
    async function deleteOne(req: ChiselRequest): Promise<Response> {
        const id = req.params.get('id');
        await entity.delete({ id });
        return createResponse(`Deleted ID ${id}`, 200);
    }
    if (config?.deleteOne ?? true)
        routeMap.delete('/:id', deleteOne);

    return routeMap;
}

async function fetchEntitiesCrud<T extends ChiselEntity>(
    type: { new (): T },
    urlPath: string,
    urlQuery: [string, string][],
): Promise<T[]> {
    const results = await opAsync(
        "op_chisel_crud_query",
        {
            typeName: type.name,
            urlPath,
            urlQuery,
        },
        requestContext,
    );
    return results as T[];
}

async function deleteEntitiesCrud<T extends ChiselEntity>(
    type: { new (): T },
    urlQuery: [string, string][],
): Promise<void> {
    await opAsync(
        "op_chisel_crud_delete",
        {
            typeName: type.name,
            urlQuery,
        },
        requestContext,
    );
}

