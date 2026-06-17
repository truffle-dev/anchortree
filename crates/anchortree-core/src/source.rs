//! The observation seam that keeps the core browser-free.
//!
//! [`ObservationSource`] is the single boundary between the pure identity
//! engine and the messy world of driving a real browser. Anything that can
//! produce one interactive-element pass implements it: the `anchortree-cdp`
//! crate wraps a live CDP page, while tests and benches can hand back a canned
//! `Vec<ObservedNode>`. A consumer can therefore write the whole agent loop
//! against `ObservationSource` + [`IdentityMap`](crate::IdentityMap) without
//! ever depending on a browser crate.
//!
//! The method is `async` because the only real implementation does network
//! round-trips over a CDP WebSocket. Defining it with a return-position
//! `impl Future` keeps this crate dependency-free: no `async-trait`, no
//! runtime, just `core::future::Future`.

use core::future::Future;

use crate::identity::ObservedNode;

/// A source of observation passes for [`IdentityMap`](crate::IdentityMap).
///
/// One call to [`observe`](ObservationSource::observe) corresponds to one
/// fused accessibility + DOM + layout snapshot of the page, returned as the
/// flat `Vec<ObservedNode>` the identity engine consumes. The source is
/// responsible for everything browser-specific (CDP transport, AX-tree
/// fetching, layout fusion); the engine is responsible for identity and diff.
pub trait ObservationSource {
    /// The error a failed observation can produce (transport failure, a
    /// malformed CDP reply, and so on).
    type Error;

    /// Run one observation pass and return the interactive-element set.
    ///
    /// Implementations should return the elements they consider worth a
    /// durable identity (see [`Role::is_interactive`](crate::Role::is_interactive)),
    /// already fused with layout. The order is not significant; the engine keys
    /// everything on `backend_node_id` and fingerprint.
    fn observe(&mut self) -> impl Future<Output = Result<Vec<ObservedNode>, Self::Error>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::{Bbox, Fingerprint};
    use crate::identity::{ElementState, IdentityMap};
    use crate::role::Role;

    /// A canned source proving the trait composes with the engine without a
    /// browser in sight.
    struct CannedSource {
        passes: std::vec::IntoIter<Vec<ObservedNode>>,
    }

    impl ObservationSource for CannedSource {
        type Error = std::convert::Infallible;

        async fn observe(&mut self) -> Result<Vec<ObservedNode>, Self::Error> {
            Ok(self.passes.next().unwrap_or_default())
        }
    }

    fn one_button() -> Vec<ObservedNode> {
        vec![ObservedNode {
            backend_node_id: 1,
            fingerprint: Fingerprint {
                stable_attr: Some("submit".into()),
                role: Role::Button,
                accessible_name: "Sign in".into(),
                structural_path: "form>button:1".into(),
                centroid: (50.0, 12.0),
            },
            bbox: Bbox {
                x: 10.0,
                y: 0.0,
                w: 80.0,
                h: 24.0,
            },
            state: ElementState {
                enabled: true,
                visible: true,
                ..Default::default()
            },
            text: "Sign in".into(),
        }]
    }

    #[test]
    fn source_drives_the_identity_map() {
        // A trivial hand-rolled executor so the test needs no async runtime
        // dependency: poll the future to completion on the current thread.
        fn block_on<F: Future>(mut fut: F) -> F::Output {
            use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
            fn noop(_: *const ()) {}
            fn clone(_: *const ()) -> RawWaker {
                RawWaker::new(core::ptr::null(), &VTABLE)
            }
            static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
            let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
            let mut cx = Context::from_waker(&waker);
            // Safety: the future never moves; it is owned and pinned on-stack.
            let mut fut = unsafe { core::pin::Pin::new_unchecked(&mut fut) };
            loop {
                if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                    return v;
                }
            }
        }

        let mut src = CannedSource {
            passes: vec![one_button(), one_button()].into_iter(),
        };
        let mut map = IdentityMap::new();

        let first = block_on(src.observe()).unwrap();
        let d1 = map.observe(first);
        assert_eq!(d1.added.len(), 1, "first pass mints the button");

        let second = block_on(src.observe()).unwrap();
        let d2 = map.observe(second);
        assert!(d2.is_empty(), "unchanged pass yields an empty diff");
    }
}
