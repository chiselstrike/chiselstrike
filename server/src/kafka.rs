// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

/*
use crate::api::ApiService;
use crate::server::DoRepeat;
use anyhow::Result;
use deno_core::futures::{self, stream::StreamExt};
use rskafka::client::{
    consumer::{StartOffset, StreamConsumerBuilder},
    ClientBuilder,
};
use std::rc::Rc;
use std::sync::Arc;

pub(crate) async fn spawn(
    api: Rc<ApiService>,
    connection: String,
    topics: Vec<String>,
    shutdown: async_channel::Receiver<()>,
) -> Result<Vec<tokio::task::JoinHandle<Result<()>>>> {
    let client = ClientBuilder::new(vec![connection]).build().await?;
    let streams = topics.iter().map(move |topic| {
        let topic = topic.clone();
        let partition_client = Arc::new(client.partition_client(topic.clone(), 0).unwrap());
        let stream = StreamConsumerBuilder::new(partition_client, StartOffset::Latest)
            .with_max_wait_ms(100)
            .build();
        stream.map(move |record| (topic.clone(), record))
    });
    let mut streams = futures::stream::select_all(streams);
    let mut tasks = Vec::new();
    let task = tokio::task::spawn_local(async move {
        loop {
            let ret = tokio::select! {
                _ = shutdown.recv() => DoRepeat::No,
                topic_record_offset_opt = streams.next() => {
                    let (topic, record_offset_opt) = topic_record_offset_opt.expect("some record");
                    let (record, _) = record_offset_opt.expect("no error");
                    api.handle_event(topic, record.record.key, record.record.value).await?;
                    DoRepeat::Yes
                },
            };
            if matches!(ret, DoRepeat::No) {
                break;
            }
        }
        Ok(())
    });
    tasks.push(task);
    Ok(tasks)
}

pub(crate) async fn init() -> Result<()> {
    Ok(())
}

pub(crate) fn shutdown() {}
*/
