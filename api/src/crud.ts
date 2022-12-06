// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>
import { opAsync, responseFromJson } from "./utils.ts";
import { ChiselEntity, mergeIntoEntity, requestContext } from "./datastore.ts";
import { ChiselRequest } from "./request.ts";
import { RouteMap } from "./routing.ts";
import { ClientMetadata, CrudHandler } from "./routing.ts";

export type ChiselEntityClass<T extends ChiselEntity> = {
    new (): T;
    findOne: (_: { id: string }) => Promise<T | undefined>;
    findMany: (_: Partial<T>) => Promise<T[]>;
    build: (...properties: Record<string, unknown>[]) => T;
    delete: (_: { id: string }) => Promise<void>;
};

/**
 * Generates a route map to handle REST methods GET/PUT/POST/DELETE for this entity.
 * @example
 * Put this in the file 'routes/comments.ts':
 * ```typescript
 * import { Comment } from "../models/comment";
 * export default crud(Comment);
 * ```
 * This results in a /comments endpoint that correctly handles all REST methods over Comment.
 * @param entity Entity type
 * @param config Configure the CRUD behavior. Using this parameter is currently experimental, this API is not
 * yet stabilized.
 *  - `createResponse`: function to create response from entity or an array of entities. Defaults to
 *    `responseFromJson()`.
 *  - `getAll`: should we generate `GET /` route that returns all entities according to filters in the URL
 *    query parameters? Defaults to true.
 *  - `getOne`: should we generate `GET /:id` route that returns one entity by id? Defaults to true.
 *  - `write`: should we generate the routes that write to the database? Defaults to true, and it can be
 *    overriden on a per-route basis.
 *  - `post`: should we generate `POST /` route that creates a new entity? Defaults to the value of `write`.
 *  - `put`: should we generate `PUT /:id` route that creates or updates an entity? Defaults to the
 *    value of `write`.
 *  - `patch`: should we generate `PATCH /:id` route that modifies an entity? Defaults to the value of `write`.
 *  - `deleteAll`: should we generate `DELETE /` route that deletes all entities according to filters in the
 *    URL query parameters? Defaults to the value of `write`.
 *  - `deleteOne`: should we generate `DELETE /:id` route that deletes one entity by id? Defaults to the
 *    value of `write`.
 * @returns A route map suitable as a default export in a route file.
 */
export function crud<
    T extends ChiselEntity,
    E extends ChiselEntityClass<T>,
>(
    entity: E,
    config?: {
        createResponse?: (
            body: unknown,
            status: number,
        ) => Promise<Response> | Response;
        getAll?: boolean;
        getOne?: boolean;
        write?: boolean;
        post?: boolean;
        put?: boolean;
        patch?: boolean;
        deleteAll?: boolean;
        deleteOne?: boolean;
    },
): RouteMap {
    const createResponse = config?.createResponse ?? responseFromJson;
    const routeMap = new RouteMap();
    const clientMetadata = (handlerName: CrudHandler): ClientMetadata => {
        return {
            handler: {
                kind: "Crud",
                handler: { kind: handlerName, entityName: entity.name },
            },
        };
    };

    // Returns all entities matching the filter in the `filter` URL parameter.
    async function getAll(req: ChiselRequest): Promise<Response> {
        return createResponse(
            await fetchEntitiesCrud(entity, req.path, Array.from(req.query)),
            200,
        );
    }
    if (config?.getAll ?? true) {
        routeMap.route("GET", "/", getAll, clientMetadata("GetMany"));
    }

    // Returns a specific entity matching :id
    async function getOne(req: ChiselRequest): Promise<Response> {
        const id = req.params.get("id");
        const u = await entity.findOne({ id });
        if (u !== undefined) {
            return createResponse(u, 200);
        } else {
            return createResponse("Not found", 404);
        }
    }
    if (config?.getOne ?? true) {
        routeMap.route("GET", "/:id", getOne, clientMetadata("GetOne"));
    }

    // Creates and returns a new entity from the `req` payload. Ignores the payload's id property and assigns a fresh one.
    async function post(req: ChiselRequest): Promise<Response> {
        const u = entity.build(await req.json());
        u.id = undefined;
        await u.save();
        return createResponse(u, 200);
    }
    if (config?.post ?? config?.write ?? true) {
        routeMap.route("POST", "/", post, clientMetadata("PostOne"));
    }

    // Updates and returns the entity matching :id from the `req` payload.
    async function put(req: ChiselRequest): Promise<Response> {
        const u = entity.build(await req.json());
        u.id = req.params.get("id");
        await u.save();
        return createResponse(u, 200);
    }
    if (config?.put ?? config?.write ?? true) {
        routeMap.route("PUT", "/:id", put, clientMetadata("PutOne"));
    }

    // Modifies an entity matching :id from the `req` payload.
    async function patch(req: ChiselRequest): Promise<Response> {
        const orig = await entity.findOne({ id: req.params.get("id") });
        if (!orig) {
            return createResponse(
                "object does not exist, cannot PATCH",
                404,
            );
        }
        mergeIntoEntity(
            entity.name,
            orig as Record<string, unknown>,
            await req.json(),
        );
        await orig.save();
        return createResponse(orig, 200);
    }
    if (config?.patch ?? config?.write ?? true) {
        routeMap.route("PATCH", "/:id", patch, clientMetadata("PatchOne"));
    }

    // Deletes all entities matching the filter in the `filter` URL parameter.
    async function deleteAll(req: ChiselRequest): Promise<Response> {
        await deleteEntitiesCrud(entity, Array.from(req.query));
        return createResponse(
            `Deleted entities matching ?${req.query.toString()}`,
            200,
        );
    }
    if (config?.deleteAll ?? config?.write ?? true) {
        routeMap.route("DELETE", "/", deleteAll, clientMetadata("DeleteMany"));
    }

    // Deletes the entity matching :id
    async function deleteOne(req: ChiselRequest): Promise<Response> {
        const id = req.params.get("id");
        await entity.delete({ id });
        return createResponse(`Deleted ID ${id}`, 200);
    }
    if (config?.deleteOne ?? config?.write ?? true) {
        routeMap.route(
            "DELETE",
            "/:id",
            deleteOne,
            clientMetadata("DeleteOne"),
        );
    }

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
        requestContext.rid,
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
        requestContext.rid,
    );
}
