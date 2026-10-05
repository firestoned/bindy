// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `shutdown.rs`

#[cfg(test)]
mod tests {
    use crate::shutdown::{channel, supervise, ShutdownSignal};
    use std::time::Duration;

    /// Long enough to show a future is still pending, short enough to keep
    /// the suite fast.
    const STILL_PENDING: Duration = Duration::from_millis(20);
    /// Ceiling for "resolved promptly" on a slow CI runner.
    const PROMPT: Duration = Duration::from_secs(2);

    fn assert_send_sync_static<T: Send + Sync + 'static>(_: &T) {}

    #[tokio::test]
    async fn signal_resolves_once_the_trigger_fires() {
        let (trigger, signal) = channel();
        let waiting = tokio::spawn(signal.wait());

        tokio::time::sleep(STILL_PENDING).await;
        assert!(!waiting.is_finished(), "nothing fired yet");
        assert!(!signal.is_triggered());

        trigger.fire();
        tokio::time::timeout(PROMPT, waiting)
            .await
            .expect("resolves after fire")
            .expect("task did not panic");
        assert!(signal.is_triggered());
    }

    #[tokio::test]
    async fn every_clone_of_the_signal_resolves() {
        let (trigger, signal) = channel();
        let clones: Vec<ShutdownSignal> = (0..3).map(|_| signal.clone()).collect();
        trigger.fire();
        for clone in clones {
            tokio::time::timeout(PROMPT, clone.wait())
                .await
                .expect("each clone resolves");
        }
    }

    #[tokio::test]
    async fn firing_twice_is_harmless() {
        let (trigger, signal) = channel();
        trigger.fire();
        trigger.fire();
        tokio::time::timeout(PROMPT, signal.wait())
            .await
            .expect("still resolved");
    }

    #[tokio::test]
    async fn dropping_the_trigger_counts_as_shutdown() {
        let (trigger, signal) = channel();
        drop(trigger);
        tokio::time::timeout(PROMPT, signal.wait())
            .await
            .expect("no trigger left means nobody can keep the controllers running");
    }

    #[test]
    fn the_wait_future_fits_graceful_shutdown_on() {
        // kube's `Controller::graceful_shutdown_on` takes
        // `impl Future<Output = ()> + Send + Sync + 'static`.
        let (_trigger, signal) = channel();
        let fut = signal.wait();
        assert_send_sync_static(&fut);
    }

    #[tokio::test]
    async fn supervise_treats_a_return_after_shutdown_as_a_drain() {
        let (trigger, signal) = channel();
        trigger.fire();
        let result = supervise("Test", async { Ok(()) }, signal).await;
        assert!(result.is_ok(), "a drained controller is a clean exit");
    }

    #[tokio::test]
    async fn supervise_treats_an_early_return_as_a_failure() {
        let (_trigger, signal) = channel();
        let result = supervise("Test", async { Ok(()) }, signal).await;
        let err = result.expect_err("a controller must not stop on its own");
        assert!(
            err.to_string().contains("Test") && err.to_string().contains("unexpectedly"),
            "names the controller: {err}"
        );
    }

    #[tokio::test]
    async fn supervise_passes_errors_through_with_the_controller_name() {
        let (_trigger, signal) = channel();
        let result = supervise("Test", async { Err(anyhow::anyhow!("boom")) }, signal).await;
        let err = result.expect_err("errors propagate");
        assert!(format!("{err:#}").contains("Test"), "{err:#}");
        assert!(format!("{err:#}").contains("boom"), "{err:#}");
    }
}
