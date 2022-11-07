// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { handleHttpRequest } from "./http.ts";
import type { HttpRequest } from "./http.ts";
import { handleKafkaEvent, TopicMap } from "./kafka.ts";
import type { KafkaEvent } from "./kafka.ts";
import { Router } from "./routing.ts";
import { RouteMap } from "./routing.ts";
import type { RouteMapLike } from "./routing.ts";
import { specialAfter, specialBefore } from "./special.ts";
import { opAsync, opSync } from "./utils.ts";
import { requestContext } from "./datastore.ts";
import { DirtyEntityError, PermissionDeniedError } from "./policies.ts";

// A generic job that we receive from Rust
type AcceptedJob =
    | { type: "http"; request: HttpRequest; ctxRid: number }
    | { type: "kafka"; event: KafkaEvent; ctxRid: number }
    | { type: "outbox"; ctxRid: number };

// This is the entry point into the TypeScript runtime, called from `main.js`
// with structures that describe the user-defined behavior (such as how to
// handle HTTP requests).
//
// The async function returns when there are no more jobs to handle.
export default async function run(
    userRouteMap: RouteMapLike,
    userTopicMap: TopicMap | undefined,
): Promise<void> {
    // build the root RouteMap from the map provided by the user and a few internal routes
    const routeMap = new RouteMap();
    specialBefore(routeMap);
    routeMap.prefix("/", RouteMap.convert(userRouteMap));
    specialAfter(routeMap);
    const router = new Router(routeMap);

    // subscribe to all requested Kafka topics
    const topicMap = userTopicMap ?? new TopicMap();
    for (const topic in topicMap.topics) {
        opSync("op_chisel_subscribe_topic", topic);
    }

    const workerIdx = Deno.core.ops.op_chisel_get_worker_idx();

    // signal to Rust that we are ready to handle jobs
    opSync("op_chisel_ready");

    // register new error class
    // @ts-ignore: Dynamic property
    Deno.core.registerErrorClass(
        "PermissionDeniedError",
        PermissionDeniedError,
    );
    // @ts-ignore: Dynamic property
    Deno.core.registerErrorClass("DirtyEntityError", DirtyEntityError);

    for (;;) {
        const job = await opAsync(
            "op_chisel_accept_job",
        ) as (AcceptedJob | null);

        // at the moment, it is impossible to handle multiple jobs concurrently, because the data layer
        // requires some global state (the `requestContext` variable in JavaScript and the transaction in
        // Rust)

        if (job === null) {
            break;
        } else if (job.type == "http") {
            requestContext.rid = job.ctxRid;
            const httpResponse = await handleHttpRequest(
                router,
                job.request,
            );
            opSync("op_chisel_http_respond", requestContext.rid, httpResponse);
        } else if (job.type == "kafka") {
            requestContext.rid = job.ctxRid;
            await handleKafkaEvent(topicMap, job.event);
        } else if (job.type == "outbox") {
            if (workerIdx == 0) {
                await opAsync("op_chisel_poll_outbox", job.ctxRid);
            }
        } else {
            throw new Error("Unknown type of AcceptedJob");
        }
        if (requestContext.rid !== undefined) {
            Deno.core.close(requestContext.rid);
            requestContext.rid = undefined;
        }
    }
}

// TODO: explore what this does in more detail
Deno.core.ops.op_set_promise_reject_callback(
    (type: number, _promise: unknown, reason: unknown) => {
        if (type == 0) { // PromiseRejectWithNoHandler
            // Without this function deno pushes the exception to
            // pending_promise_exceptions, which eventually causes an unlucky
            // user of poll_event_loop to get an error. Since user code can
            // create and reject a promise that lacks a handler, we have to do
            // this. Throwing in here causes deno to at least log the stack.
            throw new Error("Promise rejected without a handler: " + reason);
        }
    },
);
