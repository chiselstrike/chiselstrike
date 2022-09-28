// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::prelude::*;
use rskafka::{
    client::{
        ClientBuilder,
        partition::Compression,
    },
    record::Record,
};
use time::OffsetDateTime;
use std::collections::BTreeMap;

#[chisel_macros::test(modules = Node)]
pub async fn test_kafka_apply(c: TestContext) {
    c.chisel.write(
        "events/test-topic.ts",
        r##"
        import { ChiselEvent } from "@chiselstrike/api";

        export default async function (event: ChiselEvent) {
            console.log(event);
        }
    "##,
    );

    c.chisel
        .apply()
        .await
        .expect("Event handler defined: /dev/test-topic");
}

#[chisel_macros::test(modules = Node)]
pub async fn test_kafka_consume(c: TestContext) {
    if let Some(kafka_connection) = c.kafka_connection {
        let kafka_topic = c.kafka_topic.unwrap();
        c.chisel.write(
            "models/event.ts",
            r##"
            import { ChiselEntity } from '@chiselstrike/api';
            export class Event extends ChiselEntity {
                value: string;
            }
        "##,
        );
        c.chisel.write(
            "routes/events.ts",
            r##"
            import { Event } from "../models/event.ts";
            export default Event.crud();
        "##,
        );
        c.chisel.write(
            &format!("events/{}.ts", kafka_topic),
            r##"
            import { ChiselEvent } from "@chiselstrike/api";
            import { Event } from "../models/event.ts";
    
            export default async function (event: ChiselEvent) {
                const value = await event.value.text();
                await Event.create({ value });
            }
        "##,
        );
    
        let expected = format!("Event handler defined: /dev/{}", kafka_topic);
        c.chisel
            .apply()
            .await
            .expect(&expected);
    
        let client = ClientBuilder::new(vec![kafka_connection])
            .build()
            .await
            .unwrap();
        let partition_client = client.partition_client(kafka_topic.clone(), 0).unwrap();
        let record = Record {
            key: None,
            value: Some(b"hello kafka".to_vec()),
            headers: BTreeMap::from([]),
            timestamp: OffsetDateTime::now_utc(),
        };
        partition_client
            .produce(vec![record], Compression::default())
            .await
            .unwrap();
        let response = c.chisel.get("/dev/events")
            .send_retry(|resp| {
                !resp.json()["results"].as_array().unwrap().is_empty()
            })
            .await
            .json();
        assert_eq!("hello kafka", response["results"][0]["value"]);
    }
}
