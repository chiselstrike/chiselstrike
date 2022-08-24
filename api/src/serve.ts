// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { handleHttpRequest } from "./http.ts";
import type { HttpRequest } from "./http.ts";
import { handleKafkaEvent } from "./kafka.ts";
import type { KafkaEvent, TopicMap } from "./kafka.ts";
import { Router } from "./routing.ts";
import type { RouteMap } from "./routing.ts";
import { opAsync, opSync } from "./utils.ts";

// A generic job that we receive from Rust
type AcceptedJob =
    | { type: "http"; request: HttpRequest; responseRid: number }
    | { type: "kafka"; event: KafkaEvent };

export async function serve(
    routeMap: RouteMap,
    topicMap: TopicMap,
): Promise<void> {
    const router = new Router(routeMap);
    Deno.core.opSync("op_chisel_ready");

    for (;;) {
        const job = await opAsync(
            "op_chisel_accept_job",
        ) as (AcceptedJob | null);
        if (job === null) {
            break;
        } else if (job.type == "http") {
            const httpResponse = await handleHttpRequest(router, job.request);
            opSync("op_chisel_http_respond", job.responseRid, httpResponse);
        } else if (job.type == "kafka") {
            await handleKafkaEvent(topicMap, job.event);
        } else {
            throw new Error("Unknown type of AcceptedJob");
        }
    }
}
