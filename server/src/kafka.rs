// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::nursery::{Nursery, NurseryStream};
use crate::server::Server;
use crate::version::VersionJob;
use anyhow::Result;
use deno_core::serde_v8;
use enclose::enclose;
use futures::stream::{FuturesUnordered, StreamExt, TryStreamExt};
use parking_lot::Mutex;
use rskafka::client::{
    consumer::{StartOffset, StreamConsumerBuilder},
    partition::{Compression, PartitionClient},
    Client, ClientBuilder,
};
use rskafka::record::Record;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use time::OffsetDateTime;
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
    topic_nursery: Nursery<TaskHandle<Result<()>>>,
    topic_stream: Mutex<Option<NurseryStream<TaskHandle<Result<()>>>>>,
    // The `outbox_poll_mutex` is used to serialize concurrent calls to outbox
    // polling to avoid publishing events from outbox multiple times.
    pub(crate) outbox_poll_mutex: async_lock::Mutex<()>,
}

impl KafkaService {
    pub async fn connect(connection: &str) -> Result<KafkaService> {
        let client = ClientBuilder::new(vec![connection.to_owned()])
            .build()
            .await?;
        let topics = Mutex::new(HashMap::default());
        let (topic_nursery, topic_stream) = Nursery::new();
        Ok(KafkaService {
            client,
            topics,
            topic_nursery,
            topic_stream: Mutex::new(Some(topic_stream)),
            outbox_poll_mutex: async_lock::Mutex::new(()),
        })
    }

    pub async fn publish_event(
        &self,
        topic: &str,
        key: Option<Vec<u8>>,
        value: Option<Vec<u8>>,
    ) -> Result<()> {
        let partition_client = Arc::new(self.client.partition_client(topic.to_owned(), 0)?);
        let record = Record {
            key,
            value,
            headers: BTreeMap::default(),
            timestamp: OffsetDateTime::now_utc(),
        };
        partition_client
            .produce(vec![record], Compression::default())
            .await?;
        Ok(())
    }

    pub fn subscribe_topic(&self, server: Arc<Server>, topic: String) {
        let mut topics = self.topics.lock();
        if topics.contains_key(&topic) {
            return;
        }
        let partition_client = Arc::new(self.client.partition_client(topic.clone(), 0).unwrap());
        topics.insert(topic.clone(), partition_client.clone());
        self.topic_nursery
            .spawn(handle_topic(server, partition_client, topic));
    }

    pub async fn publish(&self, server: Arc<Server>) -> Result<()> {
        handle_publish(server).await
    }
}

pub async fn spawn(service: Arc<KafkaService>) -> Result<TaskHandle<Result<()>>> {
    let stream = service
        .topic_stream
        .lock()
        .take()
        .expect("trying to spawn a KafkaService multiple times");
    let task = tokio::task::spawn(stream.try_collect());
    Ok(TaskHandle(task))
}

async fn handle_topic(
    server: Arc<Server>,
    client: Arc<PartitionClient>,
    topic: String,
) -> Result<()> {
    let mut stream = StreamConsumerBuilder::new(client, StartOffset::Latest)
        .with_max_wait_ms(100)
        .build();
    while let Some(event) = stream.next().await {
        match event {
            Ok((record_and_offset, _)) => {
                handle_event(&server, topic.clone(), record_and_offset.record).await?;
            }
            Err(err) => {
                warn!("Failed to receive Kafka event: {}", err);
            }
        }
    }
    Ok(())
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

async fn handle_publish(server: Arc<Server>) -> Result<()> {
    let send_futs = server
        .trunk
        .list_trunk_versions()
        .into_iter()
        .map(|trunk_version| {
            enclose! {() async move {
                let job = VersionJob::Outbox;
                let _: Result<_, _> = trunk_version.job_tx.send(job).await;
            }}
        })
        .collect::<FuturesUnordered<_>>();
    send_futs.collect::<()>().await;

    Ok(())
}
