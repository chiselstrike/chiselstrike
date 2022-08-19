// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::rcmut::RcMut;
use derive_new::new;
use once_cell::sync::OnceCell;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(new)]
pub struct Runtime {
    pub api: Rc<ApiService>,
}

thread_local!(static RUNTIME: OnceCell<Rc<RefCell<Runtime>>> = OnceCell::new());

pub fn set(rt: Runtime) {
    RUNTIME.with(|x| {
        x.set(Rc::new(RefCell::new(rt)))
            .map_err(|_| ())
            .expect("Runtime is already initialized.");
    })
}

pub fn get() -> RcMut<Runtime> {
    RUNTIME.with(|x| {
        let rc = x.get().expect("Runtime is not yet initialized.").clone();
        RcMut::new(rc)
    })
}
