// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Result};
use std::{collections::HashMap, sync::RwLock};

use once_cell::sync::Lazy;
use prometheus::{
    core::{AtomicF64, GenericCounter},
    CounterVec, Encoder, IntGaugeVec, Opts, Registry, TextEncoder,
};

pub(crate) struct Metrics {
    reg: Registry,
    counters: RwLock<HashMap<String, CounterVec>>,
}

impl Metrics {
    pub(crate) fn new() -> Metrics {
        let gauge_opts = Opts::new("version", "Currently running version of chiseld");
        let version = IntGaugeVec::new(gauge_opts, &["version"]).unwrap();
        version
            .with_label_values(&[env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT")])
            .set(1);

        let reg = Registry::new();
        reg.register(Box::new(version)).unwrap();

        let counters = RwLock::new(HashMap::default());
        Self { reg, counters }
    }

    // FIXME: stream directly to body?
    pub(crate) fn gather(&self) -> String {
        let mut buffer = vec![];
        let encoder = TextEncoder::new();
        let metric_families = self.reg.gather();
        encoder.encode(&metric_families, &mut buffer).unwrap();

        String::from_utf8(buffer).unwrap()
    }

    pub(crate) fn register(&self, name: &str, help: &str, labels: &[&str]) -> Result<()> {
        let counter_opts = Opts::new(name, help);
        let counter = CounterVec::new(counter_opts, labels).unwrap();

        let mut counters = self
            .counters
            .write()
            .map_err(|_| anyhow!("Failed to aquire metrics write lock."))?;

        if !counters.contains_key(name) {
            self.reg.register(Box::new(counter.clone())).unwrap();
            counters.insert(name.into(), counter);
        }

        Ok(())
    }

    pub(crate) fn inc_by(&self, name: &str, labels: &[&str], v: f64) -> Result<()> {
        self.get_counter(name, labels)?.inc_by(v);
        Ok(())
    }

    fn get_counter(&self, name: &str, labels: &[&str]) -> Result<GenericCounter<AtomicF64>> {
        Ok(self
            .counters
            .read()
            .map_err(|_| anyhow!("Failed to aquire metrics read lock."))?
            .get(name)
            .ok_or_else(|| anyhow!("Could not find metric counter {name}."))?
            .with_label_values(labels))
    }
}

pub(crate) static GLOBAL_METRICS: Lazy<Metrics> = Lazy::new(Metrics::new);
