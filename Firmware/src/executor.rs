// This is an implementation of an asynchronous executor.
// It is only safe on single core systems.

use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

static VTABLE: RawWakerVTable = {
    unsafe fn clone(data: *const ()) -> RawWaker {
        RawWaker::new(data, &VTABLE)
    }
    unsafe fn wake_by_ref(awake_ptr: *const ()) {
        // unwrap: This method can only be called once we have an
        // initialized `Executor` so invariants of `as_ref` are upheld.
        let awake = awake_ptr.cast::<AtomicBool>().as_ref().unwrap();
        awake.store(true, Ordering::SeqCst);
    }
    unsafe fn wake(data: *const ()) {
        wake_by_ref(data);
    }
    unsafe fn drop(_: *const ()) {}
    RawWakerVTable::new(clone, wake, wake_by_ref, drop)
};

pub struct Executor<F: Future> {
    awake: AtomicBool,
    future: F,
}

impl<F: Future> Executor<F> {
    pub fn new(future: F) -> Self {
        Self {
            awake: AtomicBool::new(false),
            future,
        }
    }
    /// Run `future` to completion and then hang.
    pub fn start(&mut self) -> ! {
        // unsafe: See contracts for `RawWaker` and `RawWakerVTable`
        let waker = unsafe {
            Waker::from_raw(RawWaker::new(
                (&self.awake as *const AtomicBool).cast::<()>(),
                &VTABLE,
            ))
        };
        let mut context = Context::from_waker(&waker);
        loop {
            if self
                .awake
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // If we are awake the flag has been cleared by the `compare_exchange`
                // and we poll the interrupt.

                // unsafe: `task` is located in the containing `Executor` which
                // cannot move since this call never returns.
                let pinned = unsafe { Pin::new_unchecked(&mut self.future) };
                match pinned.poll(&mut context) {
                    Poll::Pending => continue,
                    Poll::Ready(_) => loop {},
                }
            } else {
                // If we are asleep we wait for an external stimulus.
                unsafe { riscv::asm::wfi() };
            }
        }
    }
}
