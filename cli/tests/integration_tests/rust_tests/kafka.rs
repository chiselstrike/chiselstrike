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

#[chisel_macros::test(modules = Node, kafka_topics = 1)]
pub async fn test_kafka_consume(c: TestContext) {
    if let Some(ref kafka_connection) = c.kafka_connection {
        let kafka_topic = c.kafka_topic(0);
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
    
        let client = ClientBuilder::new(vec![kafka_connection.to_string()])
            .build()
            .await
            .unwrap();
        let partition_client = client.partition_client(kafka_topic, 0).unwrap();
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

#[chisel_macros::test(modules = Node, kafka_topics = 1)]
pub async fn test_kafka_produce_and_consume(c: TestContext) {
    if let Some(ref _kafka_connection) = c.kafka_connection {
        let kafka_topic = c.kafka_topic(0);
        c.chisel.write(
            "models/hello.ts",
            r##"
            import { ChiselEntity } from '@chiselstrike/api';
            export class Hello extends ChiselEntity {
                key: string;
                value: string;
            }
        "##,
        );
        c.chisel.write(
            "routes/hello.ts",
            r##"
            import { Hello } from "../models/hello.ts";
            export default Hello.crud();
        "##,
        );
        c.chisel.write(
            &format!("events/{}.ts", kafka_topic),
            r##"
            import { ChiselEvent } from "@chiselstrike/api";
            import { Hello } from "../models/hello.ts";

            export default async function (event: ChiselEvent) {
                const key = await event.key.text();
                const value = await event.value.text();
                await Hello.create({ key, value });
            }
        "##,
        );
        c.chisel.write(
            "routes/produce.ts",
            &format!(
                r##"
            import {{ publishEvent }} from "@chiselstrike/api";
            export default async function(_request: Request) {{
                await publishEvent({{ topic: "{}", key: "hello, key", value: "hello, value" }});
            }}
        "##,
                kafka_topic
            ),
        );
        c.chisel.apply().await.unwrap();
        c.chisel.post("/dev/produce").send().await.assert_ok();
        let response = c
            .chisel
            .get("/dev/hello")
            .send_retry(|resp| !resp.json()["results"].as_array().unwrap().is_empty())
            .await
            .json();
        assert_eq!("hello, key", response["results"][0]["key"]);
        assert_eq!("hello, value", response["results"][0]["value"]);
    }
}

#[chisel_macros::test(modules = Node, kafka_topics = 1)]
pub async fn test_kafka_produce_multiple(c: TestContext) {
    if let Some(ref _kafka_connection) = c.kafka_connection {
        let kafka_topic = c.kafka_topic(0);
        c.chisel.write(
            "models/hello.ts",
            r##"
            import { ChiselEntity } from '@chiselstrike/api';
            export class Hello extends ChiselEntity {
                seqNo: number;
                key: string;
                value: string;
            }
        "##,
        );
        c.chisel.write(
            "routes/hello.ts",
            r##"
            import { Hello } from "../models/hello.ts";
            export default Hello.crud();
        "##,
        );
        c.chisel.write(
            &format!("events/{}.ts", kafka_topic),
            r##"
            import { ChiselEvent } from "@chiselstrike/api";
            import { Hello } from "../models/hello.ts";

            export default async function (event: ChiselEvent) {
                const seqNo = await Hello.cursor().count();
                const key = await event.key.text();
                const value = await event.value.text();
                await Hello.create({ seqNo, key, value });
            }
        "##,
        );
        c.chisel.write(
            "routes/produce.ts",
            &format!(
                r##"
            import {{ publishEvent }} from "@chiselstrike/api";
            export default async function(request: Request) {{
                const json = await request.json();
                const hello = json.hello;
                await publishEvent({{ topic: "{}", key: "hello, key", value: hello }});
            }}
        "##,
                kafka_topic
            ),
        );
        c.chisel.apply().await.unwrap();
        for i in 0..10 {
            let hello = format!("Hello, {}", i);
            c.chisel.post_json("/dev/produce", json!({ "hello": hello })).await;
        }
        let response = c
            .chisel
            .get("/dev/hello")
            .send_retry(|resp| {
                resp.json()["results"].as_array().unwrap().len() >= 10
            }).await
            .json();
        let mut results = response["results"].as_array().unwrap().clone();
        assert_eq!(10, results.len());
        results.sort_by(|a, b| {
            a["seqNo"].as_f64().partial_cmp(&b["seqNo"].as_f64()).unwrap()
        });
        for (i, result) in results.iter().enumerate() {
            let hello = format!("Hello, {}", i);
            assert_eq!("hello, key", result["key"]);
            assert_eq!(hello, result["value"]);
        }
    }
}
