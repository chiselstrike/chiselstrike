// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::internal::is_stopping;
use crate::server::Server;
use crate::version::VersionJob;
use anyhow::{anyhow, Result};
use deno_core::serde_v8;
use enclose::enclose;
use futures::stream::{AbortHandle, Abortable, FuturesUnordered, StreamExt, TryStreamExt};
use futures::FutureExt;
use parking_lot::Mutex;
use rskafka::client::{
    consumer::{StartOffset, StreamConsumerBuilder},
    partition::PartitionClient,
    Client, ClientBuilder,
};
use rskafka::record::Record;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Notify;
use utils::TaskHandle;

/// Kafka event that is passed to JavaScript.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KafkaEvent {
    pub topic: String,
    pub key: serde_v8::ZeroCopyBuf,
    pub value: serde_v8::ZeroCopyBuf,
}

pub struct KafkaService {
    client: Client,
    topics: Mutex<HashMap<String, Arc<PartitionClient>>>,
    resubscribe: Notify,
}

impl KafkaService {
    pub async fn connect(connection: &str) -> Result<KafkaService> {
        let client = ClientBuilder::new(vec![connection.to_owned()])
            .build()
            .await?;
        let topics = Mutex::new(HashMap::default());
        let resubscribe = Notify::new();
        Ok(KafkaService {
            client,
            topics,
            resubscribe,
        })
    }

    pub async fn subscribe_topic(&self, topic: &String) {
        let mut topics = self.topics.lock();
        if topics.contains_key(topic) {
            return;
        }
        let partition_client = Arc::new(self.client.partition_client(topic.clone(), 0).unwrap());
        topics.insert(topic.to_owned(), partition_client);
        self.resubscribe.notify_one();
    }

    pub async fn get_topics(&self) -> HashMap<String, Arc<PartitionClient>> {
        let topics = self.topics.lock();
        topics.clone()
    }
}

pub async fn spawn(server: Arc<Server>) -> Result<TaskHandle<Result<()>>> {
    let task = tokio::task::spawn(async move {
        let service = server
            .kafka_service
            .as_ref()
            .ok_or_else(|| anyhow!("Internal error: Kafka is not configured."))?;
        while !is_stopping() {
            let (mut stream, abort_handle) = build_event_stream(service).fuse().await;
            loop {
                tokio::select! {
                    event = stream.next() => {
                        if let Some(Ok((topic, (record, _)))) = event {
                            handle_event(&server, topic, record.record).await?;
                        } else {
                            break;
                        }
                    },
                    _ = service.resubscribe.notified() => {
                        abort_handle.abort();
                    }
                }
            }
        }
        Ok(())
    });
    Ok(TaskHandle(task))
}

/// Event stream from multiple Kafka topics.
type EventStream = Box<
    dyn futures::Stream<
            Item = Result<
                (String, (rskafka::record::RecordAndOffset, i64)),
                rskafka::client::error::Error,
            >,
        > + Unpin
        + Send,
>;

/// Build a multiplexed event stream from all the Kafka topics.
async fn build_event_stream(service: &KafkaService) -> (Abortable<EventStream>, AbortHandle) {
    let topics = service.get_topics().await;
    let stream: EventStream = if topics.is_empty() {
        Box::new(futures::stream::pending())
    } else {
        let streams = topics.iter().map(move |(topic, partition_client)| {
            let topic = topic.clone();
            let stream = StreamConsumerBuilder::new(partition_client.clone(), StartOffset::Latest)
                .with_max_wait_ms(100)
                .build();
            stream.map_ok(move |record| (topic.clone(), record))
        });
        Box::new(futures::stream::select_all(streams))
    };
    futures::stream::abortable(stream)
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
