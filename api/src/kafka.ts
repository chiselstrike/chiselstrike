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
    key: ArrayBuffer;
    value: ArrayBuffer;
};

export type EventHandler = (event: ChiselEvent) => Promise<void>;

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
    requestContext.headers = [];
    requestContext.path = "";
    requestContext.routingPath = "";
    requestContext.userId = undefined;

    // we use `serde_v8::ZeroCopyBuf` to pass data from Rust to JavaScript, which is materialized as
    // `Uint8Array` in JavaScript, but we want to give an `ArrayBuffer` to our users
    function toBuffer(array: Uint8Array): ArrayBuffer {
        const buffer = array.buffer;
        if (array.byteOffset != 0 || array.byteLength != buffer.byteLength) {
            throw new Error(
                "Internal error, could not convert Uint8Array to ArrayBuffer losslessly",
            );
        }
        return buffer;
    }

    // create the `ChiselEvent` object
    const chiselEvent = {
        key: toBuffer(event.key),
        value: toBuffer(event.value),
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
