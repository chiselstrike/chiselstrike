// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

import { ChiselEntity, requestContext } from "./datastore.ts";
import { opAsync, opSync } from "./utils.ts";

export type KafkaEvent = {
    topic: string;
    key: Uint8Array;
    value: Uint8Array;
};

export class ChiselOutbox extends ChiselEntity {
    timestamp: Date;
    seqNo: number;
    topic: string;
    key: ArrayBuffer;
    value: ArrayBuffer;
}

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

    await opAsync("op_chisel_begin_transaction", requestContext.rid);
    try {
        await handler(chiselEvent);
        await opAsync("op_chisel_commit_transaction", requestContext.rid);
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
            opSync("op_chisel_rollback_transaction", requestContext.rid);
        } catch (e) {
            console.error(`Error when rolling back transaction: ${e}`);
        }
    }
}

export type PublishEventArgs = {
    topic: string;
    key?: string | ArrayBuffer;
    value?: string | ArrayBuffer;
};

/**
 * Publish an event on a topic.
 *
 * Note: the `publishEvent()` API guarantees at least once semantics, which
 * means that the event is guaranteed to be published on the topic one or
 * more times. However, unlike with exactly-once semantics that Kafka, for
 * example, provides, the application is required to de-duplicate events
 * in cases where processing the same event twice results in invalid
 * behavior.
 *
 * @version experimental
 */
export async function publishEvent(args: PublishEventArgs): Promise<void> {
    const timestamp = new Date();
    // TODO: Switch `seqNo` to a proper sequence when #1893 is done.
    const seqNo = await ChiselOutbox.cursor().count();
    const topic = args.topic;
    const encoder = new TextEncoder();
    const convert = (value?: string | ArrayBuffer) => {
        if (!value) {
            return undefined;
        }
        if (typeof value === "string") {
            return encoder.encode(value);
        }
        return value;
    };
    const key = convert(args.key);
    const value = convert(args.value);
    await ChiselOutbox.create({
        timestamp,
        seqNo,
        topic,
        key,
        value,
    });
    await opAsync("op_chisel_publish");
}
