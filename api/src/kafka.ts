// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { requestContext } from "./datastore.ts";
import { opAsync, opSync } from "./utils.ts";

export type KafkaEvent = {
    topic: string;
    key: Uint8Array;
    value: Uint8Array;
};

export class TopicMap {
    topics: Record<string, EventHandler>;

    constructor() {
        this.topics = {};
    }

    topic(topic: string, handler: EventHandler) {
        this.topics[topic] = handler;
    }
}

export type ChiselEvent = {
    key: Blob;
    value: Blob;
};

export type EventHandler = (event: ChiselEvent) => Promise<void>;

// Handle a Kafka event. This should only be called from `run.ts`, see the `run()` function from details.
export async function handleKafkaEvent(
    topicMap: TopicMap,
    event: KafkaEvent,
): Promise<void> {
    const handler = topicMap.topics[event.topic];
    if (handler === undefined) {
        // just ignore events on unknown topics
        return;
    }

    // fake a global request context, so that the datastore operations work in event handler
    requestContext.method = "POST";
    requestContext.userId = undefined;

    // create the `ChiselEvent` object
    const chiselEvent = {
        key: new Blob([event.key]),
        value: new Blob([event.value]),
    };

    await opAsync("op_chisel_begin_transaction");
    try {
        await handler(chiselEvent);
        await opAsync("op_chisel_commit_transaction");
    } catch (e) {
        let description = "";
        if (e instanceof Error && e.stack !== undefined) {
            description = e.stack;
        } else {
            description = "" + e;
        }
        console.error(
            `Error for Kafka topic ${event.topic}: ${description}`,
        );

        try {
            opSync("op_chisel_rollback_transaction");
        } catch (e) {
            console.error(`Error when rolling back transaction: ${e}`);
        }
    }
}
