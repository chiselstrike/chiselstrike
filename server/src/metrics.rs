// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use once_cell::sync::Lazy;
use prometheus::{Encoder, IntCounterVec, IntGaugeVec, Opts, Registry, TextEncoder};

pub(crate) struct Metrics {
    http_requests: IntCounterVec,
    kafka_events: IntCounterVec,
    app_counter: IntCounterVec,
    reg: Registry,
}

impl Metrics {
    pub(crate) fn new() -> Metrics {
        let counter_opts = Opts::new(
            "http_requests",
            "number of times a particular endpoint was called",
        );
        let http_requests = IntCounterVec::new(counter_opts, &["path", "status"]).unwrap();

        let kafka_opts = Opts::new(
            "kafka_events",
            "number of times a particular kafka topic was processed",
        );
        let kafka_events = IntCounterVec::new(kafka_opts, &["topic"]).unwrap();

        let app_opts = Opts::new(
            "application_counter",
            "user-defined application integer counter",
        );
        let app_counter = IntCounterVec::new(app_opts, &["tag"]).unwrap();

        let gauge_opts = Opts::new("version", "Currently running version of chiseld");
        let version = IntGaugeVec::new(gauge_opts, &["version"]).unwrap();
        version
            .with_label_values(&[env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT")])
            .set(1);

        let reg = Registry::new();

        reg.register(Box::new(app_counter.clone())).unwrap();
        reg.register(Box::new(kafka_events.clone())).unwrap();
        reg.register(Box::new(http_requests.clone())).unwrap();
        reg.register(Box::new(version)).unwrap();

        Self {
            http_requests,
            kafka_events,
            app_counter,
            reg,
        }
    }

    pub(crate) fn http_request(&self, endpoint: &str, status: usize) {
        self.http_requests
            .with_label_values(&[endpoint, status.to_string().as_str()])
            .inc();
    }

    pub(crate) fn kafka_event(&self, topic: &str) {
        self.kafka_events.with_label_values(&[topic]).inc();
    }

    pub(crate) fn app_counter(&self, tag: &str) {
        self.app_counter.with_label_values(&[tag]).inc();
    }

    // FIXME: stream directly to body?
    pub(crate) fn gather(&self) -> String {
        let mut buffer = vec![];
        let encoder = TextEncoder::new();
        let metric_families = self.reg.gather();
        encoder.encode(&metric_families, &mut buffer).unwrap();

        String::from_utf8(buffer).unwrap()
    }
}

pub(crate) static GLOBAL_METRICS: Lazy<Metrics> = Lazy::new(Metrics::new);
