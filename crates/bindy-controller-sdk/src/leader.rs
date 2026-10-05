// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Leader election over a Kubernetes `Lease` (roadmap 01 Phase B step B4).
//!
//! Only one operator replica runs controllers at a time. Every replica keeps
//! its watch caches warm; the one holding the lease runs the controllers, and
//! the others wait in [`acquire_leadership`].

use anyhow::Result;
use bindy_api::constants::{
    DEFAULT_LEASE_DURATION_SECS, DEFAULT_LEASE_RENEW_DEADLINE_SECS, DEFAULT_LEASE_RETRY_PERIOD_SECS,
};
use kube::Client;
use kube_lease_manager::{LeaseManager, LeaseManagerBuilder, LeaseManagerError};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Leader election settings, from the environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderElectionConfig {
    /// Whether leader election runs at all (`BINDY_ENABLE_LEADER_ELECTION`,
    /// default `true`)
    pub enabled: bool,
    /// Lease name (`BINDY_LEASE_NAME`, default `bindy-leader`)
    pub lease_name: String,
    /// Lease namespace (`BINDY_LEASE_NAMESPACE`, else `POD_NAMESPACE`, else
    /// `bindy-system`)
    pub lease_namespace: String,
    /// This replica's identity (`POD_NAME`, else `HOSTNAME`, else
    /// `bindy-<random>`)
    pub identity: String,
    /// Lease duration in seconds (`BINDY_LEASE_DURATION_SECONDS`)
    pub lease_duration: u64,
    /// Renew deadline in seconds (`BINDY_LEASE_RENEW_DEADLINE_SECONDS`)
    pub renew_deadline: u64,
    /// Retry period in seconds (`BINDY_LEASE_RETRY_PERIOD_SECONDS`)
    pub retry_period: u64,
}

impl LeaderElectionConfig {
    /// Read the settings from the process environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Read the settings through `lookup`, so they can be tested without
    /// touching the process environment. A value that does not parse falls
    /// back to its default.
    ///
    /// # Arguments
    /// * `lookup` - Returns the value of an environment variable, if set
    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let secs = |key: &str, default: u64| {
            lookup(key)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(default)
        };

        Self {
            enabled: lookup("BINDY_ENABLE_LEADER_ELECTION")
                .and_then(|s| s.parse::<bool>().ok())
                .unwrap_or(true),
            lease_name: lookup("BINDY_LEASE_NAME").unwrap_or_else(|| "bindy-leader".to_string()),
            lease_namespace: lookup("BINDY_LEASE_NAMESPACE")
                .or_else(|| lookup("POD_NAMESPACE"))
                .unwrap_or_else(|| "bindy-system".to_string()),
            identity: lookup("POD_NAME")
                .or_else(|| lookup("HOSTNAME"))
                .unwrap_or_else(|| format!("bindy-{}", rand::random::<u32>())),
            lease_duration: secs("BINDY_LEASE_DURATION_SECONDS", DEFAULT_LEASE_DURATION_SECS),
            renew_deadline: secs(
                "BINDY_LEASE_RENEW_DEADLINE_SECONDS",
                DEFAULT_LEASE_RENEW_DEADLINE_SECS,
            ),
            retry_period: secs(
                "BINDY_LEASE_RETRY_PERIOD_SECONDS",
                DEFAULT_LEASE_RETRY_PERIOD_SECS,
            ),
        }
    }
}

/// A held lease: this replica is the leader.
pub struct Leadership {
    /// `true` while this replica leads; flips to `false` when the lease is lost
    pub leader_rx: watch::Receiver<bool>,
    /// The lease renewal task. Dropping it does not release the lease; it is
    /// kept so the task's outcome stays observable.
    pub lease_task: JoinHandle<Result<LeaseManager, LeaseManagerError>>,
}

/// Start the lease manager and wait until this replica holds the lease.
///
/// # Arguments
/// * `client` - Kubernetes client
/// * `config` - Lease settings
///
/// # Errors
/// Returns an error if the lease manager cannot be built or its status
/// channel closes before leadership is acquired.
pub async fn acquire_leadership(
    client: Client,
    config: &LeaderElectionConfig,
) -> Result<Leadership> {
    let lease_manager = LeaseManagerBuilder::new(client, &config.lease_name)
        .with_namespace(&config.lease_namespace)
        .with_identity(&config.identity)
        .with_duration(config.lease_duration)
        .with_grace(config.retry_period)
        .build()
        .await?;

    let (leader_rx, lease_task) = lease_manager.watch().await;

    let mut rx = leader_rx.clone();
    while !*rx.borrow_and_update() {
        rx.changed().await?;
    }

    Ok(Leadership {
        leader_rx,
        lease_task,
    })
}

/// Resolve when this replica stops leading.
///
/// # Arguments
/// * `leader_rx` - The receiver from [`Leadership::leader_rx`]
///
/// # Errors
/// Returns an error if the lease task ends without reporting the loss (its
/// sender is dropped), which also means this replica can no longer claim to
/// lead.
pub async fn leadership_lost(mut leader_rx: watch::Receiver<bool>) -> Result<()> {
    loop {
        leader_rx.changed().await?;
        if !*leader_rx.borrow() {
            return Ok(());
        }
    }
}

#[cfg(test)]
#[path = "leader_tests.rs"]
mod leader_tests;
