// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>
use backtrace::Backtrace;
use std::ops::Deref;
use std::sync;
use std::sync::Mutex;
use std::sync::Weak;

struct PrevNext {
    prev: Weak<DbgArcData>,
    next: Weak<DbgArcData>,
}

struct DbgArcData {
    bt: Backtrace,
    pn: Mutex<PrevNext>,
}

// We need:
// * There has to be an Arc<T> somewhere so that we can expose it.
// * The extra info has to be in an Arc too so that we can clone it
//   and it has a stable address.
pub struct Arc<T> {
    data: sync::Arc<DbgArcData>,
    // Public to provide an escape hatch for APIs that use Arc.
    pub inner: sync::Arc<T>,
}

pub struct Iter {
    cur: sync::Arc<DbgArcData>,
    end: sync::Arc<DbgArcData>,
}

impl Iterator for Iter {
    // FIXME: We could return a reference, but would probably require unsafe to implement.
    type Item = Backtrace;

    fn next(&mut self) -> std::option::Option<Self::Item> {
        let next = self.cur.pn.lock().unwrap().next.upgrade().unwrap();
        self.cur = next;
        if sync::Arc::ptr_eq(&self.cur, &self.end) {
            None
        } else {
            Some(self.cur.bt.clone())
        }
    }
}

fn insert(data: &sync::Arc<DbgArcData>, new: &sync::Arc<DbgArcData>) {
    let cur_w = sync::Arc::downgrade(data);
    let new_w = sync::Arc::downgrade(new);
    let mut new = new.pn.lock().unwrap();

    let next_w = {
        let mut cur = data.pn.lock().unwrap();
        let next_w = cur.next.clone();
        cur.next = new_w.clone();
        next_w
    };
    let next = next_w.upgrade().unwrap();
    let mut next = next.pn.lock().unwrap();

    new.prev = cur_w;
    new.next = next_w;

    next.prev = new_w;
}

fn remove(data: &sync::Arc<DbgArcData>) {
    let (prev_w, next_w) = {
        let mut this = data.pn.lock().unwrap();
        let prev = this.prev.clone();
        let next = this.next.clone();
        this.prev = Weak::new();
        this.next = Weak::new();
        (prev, next)
    };
    let next = next_w.upgrade().unwrap();
    let prev = prev_w.upgrade().unwrap();
    prev.pn.lock().unwrap().next = next_w;
    next.pn.lock().unwrap().prev = prev_w;
}

impl<T> Arc<T> {
    pub fn new(v: T) -> Arc<T> {
        let bt = Backtrace::new();
        let pn = Mutex::new(PrevNext {
            prev: Weak::new(),
            next: Weak::new(),
        });
        let data = DbgArcData { bt, pn };
        let data = sync::Arc::new(data);
        {
            let mut pn = data.pn.lock().unwrap();
            pn.prev = sync::Arc::downgrade(&data);
            pn.next = sync::Arc::downgrade(&data);
        }
        let inner = sync::Arc::new(v);
        Arc { data, inner }
    }

    fn insert(&self, bt: Backtrace) -> sync::Arc<DbgArcData> {
        let pn = Mutex::new(PrevNext {
            prev: Weak::new(),
            next: Weak::new(),
        });
        let new = sync::Arc::new(DbgArcData { bt, pn });
        insert(&self.data, &new);
        new
    }

    // Iterate over the other clones
    pub fn iter(&self) -> Iter {
        Iter {
            cur: self.data.clone(),
            end: self.data.clone(),
        }
    }

    pub fn try_unwrap(this: Arc<T>) -> Result<T, Arc<T>> {
        let inner = this.inner.clone();
        let data = this.data.clone();
        let prev = this.data.pn.lock().unwrap().prev.clone().upgrade().unwrap();
        // Drop this, otherwise Arc::try_unwrap always fails. This
        // will remove this from the list.
        drop(this);
        match sync::Arc::try_unwrap(inner) {
            Ok(v) => Ok(v),
            Err(e) => {
                // Add it back to the list, unless it was the only
                // element in the list.
                if !sync::Arc::ptr_eq(&prev, &data) {
                    insert(&prev, &data);
                }
                Err(Arc { data, inner: e })
            }
        }
    }
}

impl<T> Drop for Arc<T> {
    fn drop(&mut self) {
        remove(&self.data);
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> Clone for Arc<T> {
    fn clone(&self) -> Self {
        let data = self.insert(Backtrace::new());
        let inner = self.inner.clone();
        Arc { data, inner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let n1 = Arc::new(42);
        let n1d = n1.data.clone();
        let n1w = sync::Arc::downgrade(&n1d);
        let n2 = n1.clone();
        let n2d = n2.data.clone();
        let n2w = sync::Arc::downgrade(&n2d);
        {
            let n1 = n1d.pn.lock().unwrap();
            let n2 = n2d.pn.lock().unwrap();
            assert!(Weak::ptr_eq(&n1.next, &n2w));
            assert!(Weak::ptr_eq(&n2.prev, &n1w));

            assert!(Weak::ptr_eq(&n2.next, &n1w));
            assert!(Weak::ptr_eq(&n1.prev, &n2w));
        }

        let n3 = n1.clone();
        let n3d = n3.data.clone();
        let n3w = sync::Arc::downgrade(&n3d);
        {
            let n1 = n1d.pn.lock().unwrap();
            let n2 = n2d.pn.lock().unwrap();
            let n3 = n3d.pn.lock().unwrap();

            assert!(Weak::ptr_eq(&n1.next, &n3w));
            assert!(Weak::ptr_eq(&n3.prev, &n1w));

            assert!(Weak::ptr_eq(&n3.next, &n2w));
            assert!(Weak::ptr_eq(&n2.prev, &n3w));

            assert!(Weak::ptr_eq(&n2.next, &n1w));
            assert!(Weak::ptr_eq(&n1.prev, &n2w));
        }

        drop(n3);
        {
            let n1 = n1d.pn.lock().unwrap();
            let n2 = n2d.pn.lock().unwrap();
            assert!(Weak::ptr_eq(&n1.next, &n2w));
            assert!(Weak::ptr_eq(&n2.prev, &n1w));

            assert!(Weak::ptr_eq(&n2.next, &n1w));
            assert!(Weak::ptr_eq(&n1.prev, &n2w));
        }

        let n1 = Arc::try_unwrap(n1).unwrap_err();
        {
            let n1 = n1d.pn.lock().unwrap();
            let n2 = n2d.pn.lock().unwrap();
            assert!(Weak::ptr_eq(&n1.next, &n2w));
            assert!(Weak::ptr_eq(&n2.prev, &n1w));

            assert!(Weak::ptr_eq(&n2.next, &n1w));
            assert!(Weak::ptr_eq(&n1.prev, &n2w));
        }

        let bts: Vec<_> = n1.iter().collect();
        assert_eq!(bts.len(), 1);
        assert_eq!(format!("{:?}", bts[0]), format!("{:?}", n2d.bt));

        drop(n2);

        let external = n1.inner.clone();
        let n1 = Arc::try_unwrap(n1).unwrap_err();
        {
            let n1 = n1d.pn.lock().unwrap();
            assert!(Weak::ptr_eq(&n1.next, &n1w));
            assert!(Weak::ptr_eq(&n1.prev, &n1w));
        }

        drop(external);

        let v = Arc::try_unwrap(n1)
            .map_err(|_| "try_unwrap failed")
            .unwrap();
        assert_eq!(v, 42);
        assert_eq!(n1w.strong_count(), 1);
        assert_eq!(n1w.weak_count(), 3);
        assert_eq!(n2w.weak_count(), 1);
        assert_eq!(n3w.weak_count(), 1);
        drop(n1d);
        assert_eq!(n1w.strong_count(), 0);
    }
}
