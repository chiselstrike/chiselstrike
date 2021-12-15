// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::cell::RefCell;
use std::cell::RefMut;
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::ops::DerefMut;
use std::rc::Rc;

pub(crate) struct RcMut<T: 'static> {
    rc: Rc<RefCell<T>>,
    refmut: MaybeUninit<RefMut<'static, T>>,
}

impl<T> Drop for RcMut<T> {
    fn drop(&mut self) {
        unsafe { std::ptr::drop_in_place(self.refmut.assume_init_mut()) };
    }
}

impl<T> RcMut<T> {
    pub(crate) fn new(rc: Rc<RefCell<T>>) -> Self {
        let mut ret = Self {
            rc,
            refmut: MaybeUninit::uninit(),
        };
        let p: *const RefCell<T> = &*ret.rc;
        ret.refmut.write(unsafe { (*p).borrow_mut() });
        ret
    }
}

impl<T> Deref for RcMut<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.refmut.assume_init_ref().deref() }
    }
}

impl<T> DerefMut for RcMut<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.refmut.assume_init_mut().deref_mut() }
    }
}
