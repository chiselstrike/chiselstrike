// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

// FIXME: This is similar to the vec-map crate, but has the push
// function. We should switch to vec-map if they accept a PR adding
// push.

pub struct VecMap<V> {
    vec: Vec<Option<V>>,
}

impl<V> VecMap<V> {
    pub fn push(&mut self, v: V) -> usize {
        let ret = self.vec.len();
        self.vec.push(Some(v));
        ret
    }

    pub fn new() -> Self {
        Self { vec: vec![] }
    }

    pub fn remove(&mut self, key: usize) -> Option<V> {
        if key >= self.vec.len() {
            return None;
        }
        let ret = self.vec[key].take();
        match self.vec.iter().rposition(Option::is_some) {
            None => self.vec.clear(),
            Some(i) => self.vec.truncate(i + 1),
        };
        ret
    }
}
