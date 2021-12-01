use std::mem::ManuallyDrop;
use std::ptr;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::Ordering;

use crossbeam_epoch as epoch;
use crossbeam_epoch::{Atomic, Owned};

// Taken from crossbeam.
//
// TODO: Make elimination for the stack.
// This will perform bad.

/// Treiber's lock-free stack.
///
/// Usable with any number of producers and consumers.
#[derive(Debug)]
pub struct TreiberStack<T> {
    head: Atomic<Node<T>>,
}

#[derive(Debug)]
struct Node<T> {
    data: ManuallyDrop<T>,
    next: Atomic<Node<T>>,
}

impl<T> TreiberStack<T> {
    /// Creates a new, empty stack.
    pub fn new() -> TreiberStack<T> {
        TreiberStack {
            head: Atomic::null(),
        }
    }

    fn strongest_failure_ordering(order: Ordering) -> Ordering {
        use Ordering::*;
        match order {
            Release => Relaxed,
            Relaxed => Relaxed,
            SeqCst => SeqCst,
            Acquire => Acquire,
            AcqRel => Acquire,
            _ => unsafe { std::hint::unreachable_unchecked() }
        }
    }

    /// Pushes a value on top of the stack.
    pub fn push(&self, t: T) {
        let mut n = Owned::new(Node {
            data: ManuallyDrop::new(t),
            next: Atomic::null(),
        });

        let guard = epoch::pin();

        loop {
            let head = self.head.load(Relaxed, &guard);
            n.next.store(head, Relaxed);

            match self.head.compare_exchange(head, n, Release, Self::strongest_failure_ordering(Release), &guard) {
                Ok(_) => break,
                Err(e) => n = e.new,
            }
        }
    }

    /// Attempts to pop the top element from the stack.
    ///
    /// Returns `None` if the stack is empty.
    pub fn pop(&self) -> Option<T> {
        let guard = epoch::pin();
        loop {
            let head = self.head.load(Acquire, &guard);

            match unsafe { head.as_ref() } {
                Some(h) => {
                    let next = h.next.load(Relaxed, &guard);

                    if self
                        .head
                        .compare_exchange(head, next, Relaxed, Self::strongest_failure_ordering(Relaxed), &guard)
                        .is_ok()
                    {
                        unsafe {
                            guard.defer_destroy(head);
                            return Some(ManuallyDrop::into_inner(ptr::read(&(*h).data)));
                        }
                    }
                }
                None => return None,
            }
        }
    }

    /// Returns `true` if the stack is empty.
    pub fn is_empty(&self) -> bool {
        let guard = epoch::pin();
        self.head.load(Acquire, &guard).is_null()
    }
}
