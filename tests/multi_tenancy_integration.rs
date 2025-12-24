// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Integration tests for Multi-Tenancy Dual-Cluster Model
//!
//! These tests verify:
//! - ClusterBind9Provider (cluster-scoped) functionality
//! - Bind9Cluster (namespace-scoped) functionality
//! - Namespace isolation between tenants
//! - DNSZone references to both cluster types
//! - Cross-namespace resource access control
//!
//! Run with: cargo test --test multi_tenancy_integration -- --ignored --test-threads=1

#![allow(clippy::items_after_statements)]
#![allow(clippy::manual_let_else)]

// mod common; // Not needed for these tests

use bindy::crd::{
    Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, Bind9Instance, ClusterBind9Provider,
    ClusterBind9ProviderSpec, DNSZone, DNSZoneSpec, SOARecord, ServerRole,
};
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use kube::client::Client;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::time::sleep;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const POLLING_INTERVAL: Duration = Duration::from_secs(2);

// ============================================================================
// Test Helper Functions
// ============================================================================

/// Get Kubernetes client or skip test
async fn get_client_or_skip() -> Option<Client> {
    match Client::try_default().await {
        Ok(client) => {
            println!("✓ Connected to Kubernetes cluster");
            Some(client)
        }
        Err(e) => {
            eprintln!("⊘ Skipping test: not in Kubernetes cluster: {e}");
            None
        }
    }
}

/// Create a test namespace with labels
async fn create_namespace(client: &Client, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let namespaces: Api<Namespace> = Api::all(client.clone());

    let mut labels = BTreeMap::new();
    labels.insert("test".to_string(), "multi-tenancy".to_string());
    labels.insert(
        "managed-by".to_string(),
        "bindy-integration-test".to_string(),
    );

    let ns = Namespace {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        ..Default::default()
    };

    match namespaces.create(&PostParams::default(), &ns).await {
        Ok(_) => {
            println!("✓ Created namespace: {name}");
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  Namespace already exists: {name}");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Delete a test namespace
async fn delete_namespace(client: &Client, name: &str) {
    let namespaces: Api<Namespace> = Api::all(client.clone());
    match namespaces.delete(name, &DeleteParams::default()).await {
        Ok(_) => println!("✓ Deleted namespace: {name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  Namespace already deleted: {name}");
        }
        Err(e) => eprintln!("⚠ Failed to delete namespace {name}: {e}"),
    }
}

/// Delete all DNSZones in a namespace
async fn delete_all_zones_in_namespace(client: &Client, namespace: &str) {
    let zones: Api<DNSZone> = Api::namespaced(client.clone(), namespace);
    match zones.list(&ListParams::default()).await {
        Ok(zone_list) => {
            for zone in zone_list.items {
                if let Some(name) = &zone.metadata.name {
                    match zones.delete(name, &DeleteParams::default()).await {
                        Ok(_) => println!("✓ Deleted DNSZone: {namespace}/{name}"),
                        Err(kube::Error::Api(ae)) if ae.code == 404 => {
                            println!("  DNSZone already deleted: {namespace}/{name}");
                        }
                        Err(e) => eprintln!("⚠ Failed to delete DNSZone {namespace}/{name}: {e}"),
                    }
                }
            }
        }
        Err(e) => eprintln!("⚠ Failed to list DNSZones in namespace {namespace}: {e}"),
    }
}

/// Delete all Bind9Instances in a namespace
async fn delete_all_instances_in_namespace(client: &Client, namespace: &str) {
    let instances: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    match instances.list(&ListParams::default()).await {
        Ok(instance_list) => {
            for instance in instance_list.items {
                if let Some(name) = &instance.metadata.name {
                    match instances.delete(name, &DeleteParams::default()).await {
                        Ok(_) => println!("✓ Deleted Bind9Instance: {namespace}/{name}"),
                        Err(kube::Error::Api(ae)) if ae.code == 404 => {
                            println!("  Bind9Instance already deleted: {namespace}/{name}");
                        }
                        Err(e) => {
                            eprintln!("⚠ Failed to delete Bind9Instance {namespace}/{name}: {e}");
                        }
                    }
                }
            }
        }
        Err(e) => eprintln!("⚠ Failed to list Bind9Instances in namespace {namespace}: {e}"),
    }
}

/// Delete all Bind9Clusters in a namespace
async fn delete_all_clusters_in_namespace(client: &Client, namespace: &str) {
    let clusters: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
    match clusters.list(&ListParams::default()).await {
        Ok(cluster_list) => {
            for cluster in cluster_list.items {
                if let Some(name) = &cluster.metadata.name {
                    match clusters.delete(name, &DeleteParams::default()).await {
                        Ok(_) => println!("✓ Deleted Bind9Cluster: {namespace}/{name}"),
                        Err(kube::Error::Api(ae)) if ae.code == 404 => {
                            println!("  Bind9Cluster already deleted: {namespace}/{name}");
                        }
                        Err(e) => {
                            eprintln!("⚠ Failed to delete Bind9Cluster {namespace}/{name}: {e}");
                        }
                    }
                }
            }
        }
        Err(e) => eprintln!("⚠ Failed to list Bind9Clusters in namespace {namespace}: {e}"),
    }
}

/// Delete all resources in a namespace before deleting the namespace
/// This prevents namespaces from getting stuck in "Terminating" state
async fn cleanup_namespace(client: &Client, namespace: &str) {
    println!("Cleaning up namespace: {namespace}");

    // Delete resources in reverse dependency order:
    // 1. DNSZones (depend on clusters)
    // 2. Bind9Instances (depend on clusters)
    // 3. Bind9Clusters
    // 4. Namespace itself
    delete_all_zones_in_namespace(client, namespace).await;
    delete_all_instances_in_namespace(client, namespace).await;
    delete_all_clusters_in_namespace(client, namespace).await;

    // Wait a bit for finalizers to complete
    sleep(Duration::from_secs(2)).await;

    // Now delete the namespace
    delete_namespace(client, namespace).await;

    // Wait to see if namespace is stuck in Terminating
    sleep(Duration::from_secs(2)).await;

    // Force-delete if still exists (remove finalizers)
    let namespaces: Api<Namespace> = Api::all(client.clone());
    match namespaces.get(namespace).await {
        Ok(ns) => {
            if ns.status.as_ref().and_then(|s| s.phase.as_deref()) == Some("Terminating") {
                println!("⚠ Namespace stuck in Terminating, removing finalizers: {namespace}");
                force_delete_namespace(client, namespace).await;
            }
        }
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            // Already deleted, all good
        }
        Err(e) => {
            eprintln!("⚠ Error checking namespace status: {e}");
        }
    }
}

/// Force delete a namespace by removing its finalizers
/// This is used when a namespace is stuck in "Terminating" state
async fn force_delete_namespace(client: &Client, namespace: &str) {
    use serde_json::json;

    // Remove finalizers by setting spec.finalizers to empty array
    let patch = json!({
        "spec": {
            "finalizers": []
        }
    });

    let namespaces: Api<Namespace> = Api::all(client.clone());
    match namespaces
        .patch(
            namespace,
            &kube::api::PatchParams::default(),
            &kube::api::Patch::Merge(&patch),
        )
        .await
    {
        Ok(_) => {
            println!("✓ Removed finalizers from namespace: {namespace}");
            sleep(Duration::from_secs(1)).await;

            // Verify deletion
            match namespaces.get(namespace).await {
                Ok(_) => println!("⚠ Namespace still exists: {namespace}"),
                Err(kube::Error::Api(ae)) if ae.code == 404 => {
                    println!("✓ Namespace successfully removed: {namespace}");
                }
                Err(e) => eprintln!("⚠ Error verifying namespace deletion: {e}"),
            }
        }
        Err(e) => eprintln!("⚠ Failed to remove finalizers from {namespace}: {e}"),
    }
}

/// Create a ClusterBind9Provider (cluster-scoped)
async fn create_global_cluster(
    client: &Client,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let cluster_providers: Api<ClusterBind9Provider> = Api::all(client.clone());

    let cluster = ClusterBind9Provider {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            ..Default::default()
        },
        spec: ClusterBind9ProviderSpec {
            namespace: None, // Use operator's namespace
            common: Bind9ClusterCommonSpec {
                version: Some("9.18".to_string()),
                primary: None,
                secondary: None,
                image: None,
                config_map_refs: None,
                global: None,
                rndc_secret_refs: None,
                acls: None,
                volumes: None,
                volume_mounts: None,
            },
        },
        status: None,
    };

    match cluster_providers
        .create(&PostParams::default(), &cluster)
        .await
    {
        Ok(_) => {
            println!("✓ Created ClusterBind9Provider: {name}");
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  ClusterBind9Provider already exists: {name}");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Delete a ClusterBind9Provider
async fn delete_global_cluster(client: &Client, name: &str) {
    let cluster_providers: Api<ClusterBind9Provider> = Api::all(client.clone());
    match cluster_providers
        .delete(name, &DeleteParams::default())
        .await
    {
        Ok(_) => println!("✓ Deleted ClusterBind9Provider: {name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  ClusterBind9Provider already deleted: {name}");
        }
        Err(e) => eprintln!("⚠ Failed to delete ClusterBind9Provider {name}: {e}"),
    }
}

/// Create a Bind9Cluster (namespace-scoped)
async fn create_namespaced_cluster(
    client: &Client,
    namespace: &str,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let clusters: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);

    let cluster = Bind9Cluster {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Bind9ClusterSpec {
            common: Bind9ClusterCommonSpec {
                version: Some("9.18".to_string()),
                primary: None,
                secondary: None,
                image: None,
                config_map_refs: None,
                global: None,
                rndc_secret_refs: None,
                acls: None,
                volumes: None,
                volume_mounts: None,
            },
        },
        status: None,
    };

    match clusters.create(&PostParams::default(), &cluster).await {
        Ok(_) => {
            println!("✓ Created Bind9Cluster: {namespace}/{name}");
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  Bind9Cluster already exists: {namespace}/{name}");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Create a Bind9Instance referencing a cluster
async fn create_instance(
    client: &Client,
    namespace: &str,
    name: &str,
    cluster_ref: &str,
    role: ServerRole,
) -> Result<(), Box<dyn std::error::Error>> {
    let instances: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    let instance = Bind9Instance {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: bindy::crd::Bind9InstanceSpec {
            cluster_ref: cluster_ref.to_string(),
            role,
            replicas: Some(1),
            version: None,
            image: None,
            config_map_refs: None,
            config: None,
            primary_servers: None,
            volumes: None,
            volume_mounts: None,
            rndc_secret_ref: None,
            storage: None,
            bindcar_config: None,
        },
        status: None,
    };

    match instances.create(&PostParams::default(), &instance).await {
        Ok(_) => {
            println!("✓ Created Bind9Instance: {namespace}/{name} (cluster_ref={cluster_ref})");
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  Bind9Instance already exists: {namespace}/{name}");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Create a DNSZone with clusterRef (namespace-scoped)
async fn create_zone_with_cluster_ref(
    client: &Client,
    namespace: &str,
    name: &str,
    zone_name: &str,
    cluster_ref: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let zones: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

    let zone = DNSZone {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: DNSZoneSpec {
            zone_name: zone_name.to_string(),
            cluster_ref: Some(cluster_ref.to_string()),
            cluster_provider_ref: None,
            soa_record: SOARecord {
                primary_ns: format!("ns1.{zone_name}."),
                admin_email: format!("admin.{zone_name}."),
                serial: 2025010101,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            ttl: Some(3600),
            name_server_ips: None,
        },
        status: None,
    };

    match zones.create(&PostParams::default(), &zone).await {
        Ok(_) => {
            println!("✓ Created DNSZone: {namespace}/{name} → clusterRef={cluster_ref}");
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  DNSZone already exists: {namespace}/{name}");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Create a DNSZone with clusterProviderRef (cluster-scoped)
async fn create_zone_with_cluster_provider_ref(
    client: &Client,
    namespace: &str,
    name: &str,
    zone_name: &str,
    cluster_provider_ref: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let zones: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

    let zone = DNSZone {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: DNSZoneSpec {
            zone_name: zone_name.to_string(),
            cluster_ref: None,
            cluster_provider_ref: Some(cluster_provider_ref.to_string()),
            soa_record: SOARecord {
                primary_ns: format!("ns1.{zone_name}."),
                admin_email: format!("admin.{zone_name}."),
                serial: 2025010101,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            ttl: Some(3600),
            name_server_ips: None,
        },
        status: None,
    };

    match zones.create(&PostParams::default(), &zone).await {
        Ok(_) => {
            println!(
                "✓ Created DNSZone: {namespace}/{name} → clusterProviderRef={cluster_provider_ref}"
            );
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  DNSZone already exists: {namespace}/{name}");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Wait for a resource to exist
async fn wait_for_resource<K>(
    api: &Api<K>,
    name: &str,
    timeout: Duration,
) -> Result<K, Box<dyn std::error::Error>>
where
    K: kube::Resource + Clone + std::fmt::Debug + serde::de::DeserializeOwned,
    <K as kube::Resource>::DynamicType: Default,
{
    let start = std::time::Instant::now();
    loop {
        match api.get(name).await {
            Ok(resource) => return Ok(resource),
            Err(kube::Error::Api(ae)) if ae.code == 404 => {
                if start.elapsed() > timeout {
                    return Err(format!("Timeout waiting for resource: {name}").into());
                }
                sleep(POLLING_INTERVAL).await;
            }
            Err(e) => return Err(Box::new(e)),
        }
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
#[ignore] // Run with: cargo test --test multi_tenancy_integration -- --ignored
async fn test_clusterbind9provider_creation() {
    println!("\n=== Test: ClusterBind9Provider Creation ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let cluster_name = "test-global-cluster";

    // Create global cluster
    if let Err(e) = create_global_cluster(&client, cluster_name).await {
        panic!("Failed to create ClusterBind9Provider: {e}");
    }

    // Wait for cluster to exist
    let cluster_providers: Api<ClusterBind9Provider> = Api::all(client.clone());
    match wait_for_resource(&cluster_providers, cluster_name, TEST_TIMEOUT).await {
        Ok(cluster) => {
            println!("✓ ClusterBind9Provider exists: {cluster_name}");
            assert_eq!(cluster.metadata.name.as_deref(), Some(cluster_name));
            assert_eq!(cluster.spec.common.version, Some("9.18".to_string()));
        }
        Err(e) => panic!("Failed to verify ClusterBind9Provider: {e}"),
    }

    // Cleanup
    delete_global_cluster(&client, cluster_name).await;
    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_bind9cluster_namespace_scoped() {
    println!("\n=== Test: Bind9Cluster Namespace-Scoped ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "test-ns-cluster";
    let cluster_name = "test-cluster";

    // Setup
    if let Err(e) = create_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    if let Err(e) = create_namespaced_cluster(&client, namespace, cluster_name).await {
        panic!("Failed to create Bind9Cluster: {e}");
    }

    // Wait for cluster to exist
    let clusters: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
    match wait_for_resource(&clusters, cluster_name, TEST_TIMEOUT).await {
        Ok(cluster) => {
            println!("✓ Bind9Cluster exists: {namespace}/{cluster_name}");
            assert_eq!(cluster.metadata.name.as_deref(), Some(cluster_name));
            assert_eq!(cluster.metadata.namespace.as_deref(), Some(namespace));
            assert_eq!(cluster.spec.common.version, Some("9.18".to_string()));
        }
        Err(e) => panic!("Failed to verify Bind9Cluster: {e}"),
    }

    // Verify cluster is NOT visible from other namespaces
    let other_namespace = "default";
    let other_clusters: Api<Bind9Cluster> = Api::namespaced(client.clone(), other_namespace);
    match other_clusters.get(cluster_name).await {
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("✓ Bind9Cluster correctly isolated from other namespaces");
        }
        Ok(_) => panic!("Bind9Cluster should NOT be visible from other namespace"),
        Err(e) => panic!("Unexpected error: {e}"),
    }

    // Cleanup
    cleanup_namespace(&client, namespace).await;
    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_dnszone_with_cluster_provider_ref() {
    println!("\n=== Test: DNSZone with clusterProviderRef ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let global_cluster_name = "test-global-cluster";
    let namespace = "test-zone-global";
    let zone_name = "test-zone";

    // Setup
    if let Err(e) = create_global_cluster(&client, global_cluster_name).await {
        panic!("Failed to create ClusterBind9Provider: {e}");
    }

    if let Err(e) = create_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create DNSZone referencing global cluster
    if let Err(e) = create_zone_with_cluster_provider_ref(
        &client,
        namespace,
        zone_name,
        "example.com",
        global_cluster_name,
    )
    .await
    {
        panic!("Failed to create DNSZone: {e}");
    }

    // Wait for zone to exist
    let zones: Api<DNSZone> = Api::namespaced(client.clone(), namespace);
    match wait_for_resource(&zones, zone_name, TEST_TIMEOUT).await {
        Ok(zone) => {
            println!("✓ DNSZone exists: {namespace}/{zone_name}");
            assert_eq!(zone.spec.zone_name, "example.com");
            assert_eq!(
                zone.spec.cluster_provider_ref.as_deref(),
                Some(global_cluster_name)
            );
            assert_eq!(zone.spec.cluster_ref, None);
        }
        Err(e) => panic!("Failed to verify DNSZone: {e}"),
    }

    // Cleanup
    delete_namespace(&client, namespace).await;
    delete_global_cluster(&client, global_cluster_name).await;
    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_dnszone_with_cluster_ref() {
    println!("\n=== Test: DNSZone with clusterRef ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "test-zone-namespaced";
    let cluster_name = "test-cluster";
    let zone_name = "test-zone";

    // Setup
    if let Err(e) = create_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    if let Err(e) = create_namespaced_cluster(&client, namespace, cluster_name).await {
        panic!("Failed to create Bind9Cluster: {e}");
    }

    // Create DNSZone referencing namespace-scoped cluster
    if let Err(e) =
        create_zone_with_cluster_ref(&client, namespace, zone_name, "test.local", cluster_name)
            .await
    {
        panic!("Failed to create DNSZone: {e}");
    }

    // Wait for zone to exist
    let zones: Api<DNSZone> = Api::namespaced(client.clone(), namespace);
    match wait_for_resource(&zones, zone_name, TEST_TIMEOUT).await {
        Ok(zone) => {
            println!("✓ DNSZone exists: {namespace}/{zone_name}");
            assert_eq!(zone.spec.zone_name, "test.local");
            assert_eq!(zone.spec.cluster_ref.as_deref(), Some(cluster_name));
            assert_eq!(zone.spec.cluster_provider_ref, None);
        }
        Err(e) => panic!("Failed to verify DNSZone: {e}"),
    }

    // Cleanup
    cleanup_namespace(&client, namespace).await;
    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_namespace_isolation() {
    println!("\n=== Test: Namespace Isolation ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace_a = "tenant-a";
    let namespace_b = "tenant-b";
    let cluster_name = "isolated-cluster";
    let zone_name = "isolated-zone";

    // Setup two separate namespaces
    if let Err(e) = create_namespace(&client, namespace_a).await {
        panic!("Failed to create namespace A: {e}");
    }

    if let Err(e) = create_namespace(&client, namespace_b).await {
        panic!("Failed to create namespace B: {e}");
    }

    // Create cluster in namespace A
    if let Err(e) = create_namespaced_cluster(&client, namespace_a, cluster_name).await {
        panic!("Failed to create cluster in namespace A: {e}");
    }

    // Create zone in namespace A
    if let Err(e) = create_zone_with_cluster_ref(
        &client,
        namespace_a,
        zone_name,
        "tenant-a.local",
        cluster_name,
    )
    .await
    {
        panic!("Failed to create zone in namespace A: {e}");
    }

    // Verify cluster in namespace A is NOT visible from namespace B
    let clusters_b: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace_b);
    match clusters_b.get(cluster_name).await {
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("✓ Bind9Cluster correctly isolated between namespaces");
        }
        Ok(_) => panic!("Bind9Cluster should NOT be visible across namespaces"),
        Err(e) => panic!("Unexpected error: {e}"),
    }

    // Verify zone in namespace A is NOT visible from namespace B
    let zones_b: Api<DNSZone> = Api::namespaced(client.clone(), namespace_b);
    match zones_b.get(zone_name).await {
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("✓ DNSZone correctly isolated between namespaces");
        }
        Ok(_) => panic!("DNSZone should NOT be visible across namespaces"),
        Err(e) => panic!("Unexpected error: {e}"),
    }

    // Cleanup
    cleanup_namespace(&client, namespace_a).await;
    cleanup_namespace(&client, namespace_b).await;
    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_global_cluster_cross_namespace_access() {
    println!("\n=== Test: Global Cluster Cross-Namespace Access ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let global_cluster_name = "shared-global-cluster";
    let namespace_a = "team-a";
    let namespace_b = "team-b";

    // Setup
    if let Err(e) = create_global_cluster(&client, global_cluster_name).await {
        panic!("Failed to create ClusterBind9Provider: {e}");
    }

    if let Err(e) = create_namespace(&client, namespace_a).await {
        panic!("Failed to create namespace A: {e}");
    }

    if let Err(e) = create_namespace(&client, namespace_b).await {
        panic!("Failed to create namespace B: {e}");
    }

    // Create zones in different namespaces referencing the same global cluster
    if let Err(e) = create_zone_with_cluster_provider_ref(
        &client,
        namespace_a,
        "zone-a",
        "team-a.example.com",
        global_cluster_name,
    )
    .await
    {
        panic!("Failed to create zone in namespace A: {e}");
    }

    if let Err(e) = create_zone_with_cluster_provider_ref(
        &client,
        namespace_b,
        "zone-b",
        "team-b.example.com",
        global_cluster_name,
    )
    .await
    {
        panic!("Failed to create zone in namespace B: {e}");
    }

    // Verify both zones can reference the same global cluster
    let zones_a: Api<DNSZone> = Api::namespaced(client.clone(), namespace_a);
    let zones_b: Api<DNSZone> = Api::namespaced(client.clone(), namespace_b);

    match zones_a.get("zone-a").await {
        Ok(zone) => {
            println!("✓ Zone in namespace A references global cluster");
            assert_eq!(
                zone.spec.cluster_provider_ref.as_deref(),
                Some(global_cluster_name)
            );
        }
        Err(e) => panic!("Failed to get zone from namespace A: {e}"),
    }

    match zones_b.get("zone-b").await {
        Ok(zone) => {
            println!("✓ Zone in namespace B references global cluster");
            assert_eq!(
                zone.spec.cluster_provider_ref.as_deref(),
                Some(global_cluster_name)
            );
        }
        Err(e) => panic!("Failed to get zone from namespace B: {e}"),
    }

    // Cleanup
    cleanup_namespace(&client, namespace_a).await;
    cleanup_namespace(&client, namespace_b).await;
    delete_global_cluster(&client, global_cluster_name).await;
    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_bind9instance_references_global_cluster() {
    println!("\n=== Test: Bind9Instance References Global Cluster ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let global_cluster_name = "test-global-cluster";
    let namespace = "test-instance-global";
    let instance_name = "test-instance";

    // Setup
    if let Err(e) = create_global_cluster(&client, global_cluster_name).await {
        panic!("Failed to create ClusterBind9Provider: {e}");
    }

    if let Err(e) = create_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create instance referencing global cluster
    if let Err(e) = create_instance(
        &client,
        namespace,
        instance_name,
        global_cluster_name,
        ServerRole::Primary,
    )
    .await
    {
        panic!("Failed to create Bind9Instance: {e}");
    }

    // Verify instance exists and references global cluster
    let instances: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    match wait_for_resource(&instances, instance_name, TEST_TIMEOUT).await {
        Ok(instance) => {
            println!("✓ Bind9Instance exists: {namespace}/{instance_name}");
            assert_eq!(instance.spec.cluster_ref, global_cluster_name);
            assert_eq!(instance.spec.role, ServerRole::Primary);
        }
        Err(e) => panic!("Failed to verify Bind9Instance: {e}"),
    }

    // Cleanup
    delete_namespace(&client, namespace).await;
    delete_global_cluster(&client, global_cluster_name).await;
    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_list_cluster_providers_across_all_namespaces() {
    println!("\n=== Test: List Global Clusters Across All Namespaces ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let cluster1_name = "global-cluster-1";
    let cluster2_name = "global-cluster-2";

    // Create multiple global clusters
    if let Err(e) = create_global_cluster(&client, cluster1_name).await {
        panic!("Failed to create global cluster 1: {e}");
    }

    if let Err(e) = create_global_cluster(&client, cluster2_name).await {
        panic!("Failed to create global cluster 2: {e}");
    }

    // List all global clusters
    let cluster_providers: Api<ClusterBind9Provider> = Api::all(client.clone());
    let lp = ListParams::default();

    match cluster_providers.list(&lp).await {
        Ok(cluster_list) => {
            let test_clusters: Vec<_> = cluster_list
                .items
                .iter()
                .filter(|c| {
                    c.metadata.name.as_deref() == Some(cluster1_name)
                        || c.metadata.name.as_deref() == Some(cluster2_name)
                })
                .collect();

            println!("✓ Found {} test global clusters", test_clusters.len());
            assert!(
                test_clusters.len() >= 2,
                "Expected at least 2 global clusters"
            );

            for cluster in test_clusters {
                println!("  - {}", cluster.metadata.name.as_ref().unwrap());
            }
        }
        Err(e) => panic!("Failed to list global clusters: {e}"),
    }

    // Cleanup
    delete_global_cluster(&client, cluster1_name).await;
    delete_global_cluster(&client, cluster2_name).await;
    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_hybrid_deployment() {
    println!("\n=== Test: Hybrid Deployment (Global + Namespaced) ===\n");

    let client = match get_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let global_cluster_name = "production-dns";
    let prod_namespace = "production";
    let dev_namespace = "development";
    let dev_cluster_name = "dev-dns";

    // Setup namespaces
    if let Err(e) = create_namespace(&client, prod_namespace).await {
        panic!("Failed to create production namespace: {e}");
    }

    if let Err(e) = create_namespace(&client, dev_namespace).await {
        panic!("Failed to create development namespace: {e}");
    }

    // Create global cluster for production
    if let Err(e) = create_global_cluster(&client, global_cluster_name).await {
        panic!("Failed to create global cluster: {e}");
    }

    // Create namespace-scoped cluster for development
    if let Err(e) = create_namespaced_cluster(&client, dev_namespace, dev_cluster_name).await {
        panic!("Failed to create dev cluster: {e}");
    }

    // Create production zone using global cluster
    if let Err(e) = create_zone_with_cluster_provider_ref(
        &client,
        prod_namespace,
        "prod-zone",
        "api.example.com",
        global_cluster_name,
    )
    .await
    {
        panic!("Failed to create production zone: {e}");
    }

    // Create development zone using namespace-scoped cluster
    if let Err(e) = create_zone_with_cluster_ref(
        &client,
        dev_namespace,
        "dev-zone",
        "dev.local",
        dev_cluster_name,
    )
    .await
    {
        panic!("Failed to create development zone: {e}");
    }

    // Verify both patterns work simultaneously
    let prod_zones: Api<DNSZone> = Api::namespaced(client.clone(), prod_namespace);
    let dev_zones: Api<DNSZone> = Api::namespaced(client.clone(), dev_namespace);

    match prod_zones.get("prod-zone").await {
        Ok(zone) => {
            println!("✓ Production zone uses global cluster");
            assert_eq!(
                zone.spec.cluster_provider_ref.as_deref(),
                Some(global_cluster_name)
            );
        }
        Err(e) => panic!("Failed to verify production zone: {e}"),
    }

    match dev_zones.get("dev-zone").await {
        Ok(zone) => {
            println!("✓ Development zone uses namespaced cluster");
            assert_eq!(zone.spec.cluster_ref.as_deref(), Some(dev_cluster_name));
        }
        Err(e) => panic!("Failed to verify development zone: {e}"),
    }

    // Cleanup
    cleanup_namespace(&client, prod_namespace).await;
    cleanup_namespace(&client, dev_namespace).await;
    delete_global_cluster(&client, global_cluster_name).await;
    println!("\n✓ Test passed\n");
}
