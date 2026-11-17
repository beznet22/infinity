//! Sleeps that work on both the host and the browser.
//!
//! `tokio::time` compiles for `wasm32-unknown-unknown` but panics the first time
//! it is polled: a browser has no tokio timer driver. The core's retry backoff
//! and its sleep tools therefore cannot call `tokio::time` directly on the web
//! target. This module is the seam.
//!
//! The trait returns a boxed future instead of using `async_trait` because the
//! browser's futures are `!Send`, and `async_trait` would force a `Send` bound
//! that the wasm build cannot satisfy (the same reason the core's traits are
//! cfg-gated — decision D15).

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// A platform sleep.
pub trait SliceTimer {
    /// Resolves after `duration`. Must not panic on the browser target.
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + '_>>;
}

/// The timer for the current target.
pub fn browser_timer() -> Box<dyn SliceTimer> {
    Box::new(PlatformTimer)
}

#[cfg(not(target_family = "wasm"))]
struct PlatformTimer;

#[cfg(not(target_family = "wasm"))]
impl SliceTimer for PlatformTimer {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[cfg(target_family = "wasm")]
struct PlatformTimer;

#[cfg(target_family = "wasm")]
impl SliceTimer for PlatformTimer {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + '_>> {
        Box::pin(wasmtimer::tokio::sleep(duration))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn sleeps_at_least_the_requested_duration() {
        let started = Instant::now();
        browser_timer().sleep(Duration::from_millis(20)).await;
        assert!(started.elapsed() >= Duration::from_millis(20));
    }

    #[tokio::test]
    async fn timer_is_usable_as_a_trait_object() {
        // Creating tokio's sleep future needs a runtime, hence the async test.
        let timer: Box<dyn SliceTimer> = browser_timer();
        timer.sleep(Duration::from_millis(1)).await;
    }
}
