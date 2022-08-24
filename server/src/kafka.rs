// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::server::Server;
use crate::version::VersionJob;
use anyhow::{Context, Result};
use deno_core::serde_v8;
use enclose::enclose;
use futures::stream::{FuturesUnordered, StreamExt, TryStreamExt};
use rskafka::client::{
    consumer::{StartOffset, StreamConsumerBuilder},
    ClientBuilder,
};
use rskafka::record::Record;
use serde::Serialize;
use std::sync::Arc;
use utils::TaskHandle;

/// Kafka event that is passed to JavaScript.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KafkaEvent {
    pub topic: String,
    pub key: serde_v8::ZeroCopyBuf,
    pub value: serde_v8::ZeroCopyBuf,
}

pub async fn spawn(
    server: Arc<Server>,
    connection: String,
    topics: &[String],
) -> Result<TaskHandle<Result<()>>> {
    let client = ClientBuilder::new(vec![connection]).build().await?;
    let streams = topics.iter().map(move |topic| {
        let topic = topic.clone();
        let partition_client = Arc::new(client.partition_client(topic.clone(), 0).unwrap());
        let stream = StreamConsumerBuilder::new(partition_client, StartOffset::Latest)
            .with_max_wait_ms(100)
            .build();
        stream.map_ok(move |record| (topic.clone(), record))
    });
    let mut streams = futures::stream::select_all(streams);

    let task = tokio::task::spawn(async move {
        while let Some(res) = streams.next().await {
            let (topic, (record, _)) = res.context("Could not receive event from Kafka")?;
            handle_event(&server, topic, record.record).await?;
        }
        Ok(())
    });
    Ok(TaskHandle(task))
}

async fn handle_event(server: &Server, topic: String, record: Record) -> Result<()> {
    let key = record.key.unwrap_or_default();
    let value = record.value.unwrap_or_default();

    // TODO: this is just a dirty proof-of-concept; in particular:
    // - we don't know how to map events to versions, so we send the event to _all_ versions
    // - we don't care whether the event was handled correctly or not (we simply ignore any issues
    // with at-most-once/at-least-once semantics of event delivery)

    // send the job to all versions concurrently
    let send_futs = server
        .trunk
        .list_trunk_versions()
        .into_iter()
        .map(|trunk_version| {
            enclose! {(topic, key, value) async move {
                let kafka_event = KafkaEvent {
                    topic,
                    key: key.into(),
                    value: value.into(),
                };
                let job = VersionJob::Kafka(kafka_event);
                let _: Result<_, _> = trunk_version.job_tx.send(job).await;
            }}
        })
        .collect::<FuturesUnordered<_>>();
    send_futs.collect::<()>().await;

    Ok(())
}
