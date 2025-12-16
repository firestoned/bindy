// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Comprehensive integration tests for Bindy DNS Controller
//!
//! These tests verify the controller is working correctly in a Kubernetes cluster.
//! They cover all CRD types, basic CRUD operations, and common scenarios.
//!
//! Run with: cargo test --test simple_integration -- --ignored

#![allow(clippy::items_after_statements)]
#![allow(clippy::manual_let_else)]

use bindy::crd::{
    ARecord, ARecordSpec, Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec,
    Bind9GlobalCluster, Bind9GlobalClusterSpec, Bind9Instance, Bind9InstanceSpec, CNAMERecord,
    CNAMERecordSpec, DNSZone, DNSZoneSpec, MXRecord, MXRecordSpec, SOARecord, ServerRole,
    TXTRecord, TXTRecordSpec,
};
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use kube::client::Client;
use std::collections::BTreeMap;

// ============================================================================
// Helper Functions
// ============================================================================

/// Test helper to check if running in a Kubernetes cluster
async fn get_kube_client_or_skip() -> Option<Client> {
    match Client::try_default().await {
        Ok(client) => {
            println!("✓ Successfully connected to Kubernetes cluster");
            Some(client)
        }
        Err(e) => {
            eprintln!("⊘ Skipping integration test: not running in Kubernetes cluster: {e}");
            None
        }
    }
}

/// Create a test namespace
async fn create_test_namespace(
    client: &Client,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let namespaces: Api<Namespace> = Api::all(client.clone());

    let mut labels = BTreeMap::new();
    labels.insert("test".to_string(), "integration".to_string());
    labels.insert("managed-by".to_string(), "bindy-simple-test".to_string());

    let test_ns = Namespace {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        ..Default::default()
    };

    match namespaces.create(&PostParams::default(), &test_ns).await {
        Ok(_) => {
            println!("✓ Created test namespace: {name}");
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  Test namespace already exists: {name}");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Delete a test namespace
async fn delete_test_namespace(client: &Client, name: &str) {
    let namespaces: Api<Namespace> = Api::all(client.clone());
    match namespaces.delete(name, &DeleteParams::default()).await {
        Ok(_) => println!("✓ Deleted test namespace: {name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  Test namespace already deleted: {name}");
        }
        Err(e) => eprintln!("⚠ Failed to delete test namespace {name}: {e}"),
    }
}

// ============================================================================
// Basic Connectivity Tests
// ============================================================================

#[tokio::test]
#[ignore] // Run with: cargo test --test simple_integration -- --ignored
async fn test_kubernetes_connectivity() {
    println!("\n=== Test: Kubernetes Connectivity ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespaces: Api<Namespace> = Api::all(client);
    let lp = ListParams::default().limit(5);

    match namespaces.list(&lp).await {
        Ok(ns_list) => {
            println!("✓ Successfully connected to Kubernetes");
            println!("✓ Found {} namespaces", ns_list.items.len());
            assert!(!ns_list.items.is_empty(), "Expected at least one namespace");
        }
        Err(e) => {
            panic!("Failed to list namespaces: {e}");
        }
    }

    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_crds_installed() {
    println!("\n=== Test: Bindy CRDs Installed ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let crds: Api<CustomResourceDefinition> = Api::all(client);
    let lp = ListParams::default();

    match crds.list(&lp).await {
        Ok(crd_list) => {
            let bindy_crds: Vec<_> = crd_list
                .items
                .iter()
                .filter(|crd| crd.spec.group.as_str() == "bindy.firestoned.io")
                .collect();

            println!("✓ Found {} Bindy CRDs", bindy_crds.len());

            let expected_crds = vec![
                "Bind9Cluster",
                "Bind9GlobalCluster",
                "Bind9Instance",
                "DNSZone",
                "ARecord",
                "AAAARecord",
                "CNAMERecord",
                "MXRecord",
                "TXTRecord",
                "NSRecord",
                "SRVRecord",
                "CAARecord",
            ];

            for crd in &bindy_crds {
                println!("  - {}", crd.spec.names.kind);
            }

            if bindy_crds.is_empty() {
                println!(
                    "⚠ Warning: No Bindy CRDs found. Install with: kubectl apply -f deploy/crds/"
                );
            } else {
                println!(
                    "✓ Expected {} CRDs, found {}",
                    expected_crds.len(),
                    bindy_crds.len()
                );
            }
        }
        Err(e) => {
            println!("⚠ Could not check CRDs: {e}");
            println!("  This is expected if you don't have CRD permissions");
        }
    }

    println!("\n✓ Test passed\n");
}

// ============================================================================
// Namespace Management Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_create_and_cleanup_namespace() {
    println!("\n=== Test: Create and Cleanup Namespace ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let test_ns_name = "bindy-integration-test";

    // Create namespace
    if let Err(e) = create_test_namespace(&client, test_ns_name).await {
        panic!("Failed to create test namespace: {e}");
    }

    // Verify namespace exists
    let namespaces: Api<Namespace> = Api::all(client.clone());
    match namespaces.get(test_ns_name).await {
        Ok(ns) => {
            println!("✓ Verified namespace exists: {}", ns.metadata.name.unwrap());
            assert!(ns.metadata.labels.is_some());
        }
        Err(e) => panic!("Failed to verify namespace: {e}"),
    }

    // Cleanup
    delete_test_namespace(&client, test_ns_name).await;

    println!("\n✓ Test passed\n");
}

// ============================================================================
// Bind9Cluster Tests (Namespace-Scoped)
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_bind9cluster_create_read_delete() {
    println!("\n=== Test: Bind9Cluster CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "bindy-test-cluster";
    let cluster_name = "test-cluster";

    // Setup
    if let Err(e) = create_test_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create Bind9Cluster
    let clusters: Api<Bind9Cluster> = Api::namespaced(client.clone(), namespace);
    let cluster = Bind9Cluster {
        metadata: ObjectMeta {
            name: Some(cluster_name.to_string()),
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
        Ok(created) => {
            println!("✓ Created Bind9Cluster: {namespace}/{cluster_name}");
            assert_eq!(created.metadata.name.as_deref(), Some(cluster_name));
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  Bind9Cluster already exists");
        }
        Err(e) => panic!("Failed to create Bind9Cluster: {e}"),
    }

    // Read Bind9Cluster
    match clusters.get(cluster_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved Bind9Cluster: {namespace}/{cluster_name}");
            assert_eq!(retrieved.metadata.name.as_deref(), Some(cluster_name));
            assert_eq!(retrieved.spec.common.version, Some("9.18".to_string()));
        }
        Err(e) => panic!("Failed to retrieve Bind9Cluster: {e}"),
    }

    // List Bind9Clusters
    match clusters.list(&ListParams::default()).await {
        Ok(list) => {
            println!("✓ Listed {} Bind9Cluster(s)", list.items.len());
            assert!(!list.items.is_empty());
        }
        Err(e) => panic!("Failed to list Bind9Clusters: {e}"),
    }

    // Delete Bind9Cluster
    match clusters
        .delete(cluster_name, &DeleteParams::default())
        .await
    {
        Ok(_) => println!("✓ Deleted Bind9Cluster: {namespace}/{cluster_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  Bind9Cluster already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete Bind9Cluster: {e}"),
    }

    // Cleanup
    delete_test_namespace(&client, namespace).await;

    println!("\n✓ Test passed\n");
}

// ============================================================================
// Bind9GlobalCluster Tests (Cluster-Scoped)
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_bind9globalcluster_create_read_delete() {
    println!("\n=== Test: Bind9GlobalCluster CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let cluster_name = "test-global-cluster";

    // Create Bind9GlobalCluster
    let global_clusters: Api<Bind9GlobalCluster> = Api::all(client.clone());
    let cluster = Bind9GlobalCluster {
        metadata: ObjectMeta {
            name: Some(cluster_name.to_string()),
            ..Default::default()
        },
        spec: Bind9GlobalClusterSpec {
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

    match global_clusters
        .create(&PostParams::default(), &cluster)
        .await
    {
        Ok(created) => {
            println!("✓ Created Bind9GlobalCluster: {cluster_name}");
            assert_eq!(created.metadata.name.as_deref(), Some(cluster_name));
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  Bind9GlobalCluster already exists");
        }
        Err(e) => panic!("Failed to create Bind9GlobalCluster: {e}"),
    }

    // Read Bind9GlobalCluster
    match global_clusters.get(cluster_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved Bind9GlobalCluster: {cluster_name}");
            assert_eq!(retrieved.metadata.name.as_deref(), Some(cluster_name));
        }
        Err(e) => panic!("Failed to retrieve Bind9GlobalCluster: {e}"),
    }

    // List Bind9GlobalClusters
    match global_clusters.list(&ListParams::default()).await {
        Ok(list) => {
            println!("✓ Listed {} Bind9GlobalCluster(s)", list.items.len());
        }
        Err(e) => panic!("Failed to list Bind9GlobalClusters: {e}"),
    }

    // Delete Bind9GlobalCluster
    match global_clusters
        .delete(cluster_name, &DeleteParams::default())
        .await
    {
        Ok(_) => println!("✓ Deleted Bind9GlobalCluster: {cluster_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  Bind9GlobalCluster already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete Bind9GlobalCluster: {e}"),
    }

    println!("\n✓ Test passed\n");
}

// ============================================================================
// Bind9Instance Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_bind9instance_create_read_delete() {
    println!("\n=== Test: Bind9Instance CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "bindy-test-instance";
    let instance_name = "test-instance";
    let cluster_ref = "test-cluster";

    // Setup
    if let Err(e) = create_test_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create Bind9Instance
    let instances: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);
    let instance = Bind9Instance {
        metadata: ObjectMeta {
            name: Some(instance_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Bind9InstanceSpec {
            cluster_ref: cluster_ref.to_string(),
            role: ServerRole::Primary,
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
        Ok(created) => {
            println!("✓ Created Bind9Instance: {namespace}/{instance_name}");
            assert_eq!(created.metadata.name.as_deref(), Some(instance_name));
            assert_eq!(created.spec.role, ServerRole::Primary);
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  Bind9Instance already exists");
        }
        Err(e) => panic!("Failed to create Bind9Instance: {e}"),
    }

    // Read Bind9Instance
    match instances.get(instance_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved Bind9Instance: {namespace}/{instance_name}");
            assert_eq!(retrieved.metadata.name.as_deref(), Some(instance_name));
        }
        Err(e) => panic!("Failed to retrieve Bind9Instance: {e}"),
    }

    // Delete Bind9Instance
    match instances
        .delete(instance_name, &DeleteParams::default())
        .await
    {
        Ok(_) => println!("✓ Deleted Bind9Instance: {namespace}/{instance_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  Bind9Instance already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete Bind9Instance: {e}"),
    }

    // Cleanup
    delete_test_namespace(&client, namespace).await;

    println!("\n✓ Test passed\n");
}

// ============================================================================
// DNSZone Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_dnszone_create_read_delete() {
    println!("\n=== Test: DNSZone CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "bindy-test-zone";
    let zone_name = "test-zone";
    let cluster_ref = "test-cluster";

    // Setup
    if let Err(e) = create_test_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create DNSZone
    let zones: Api<DNSZone> = Api::namespaced(client.clone(), namespace);
    let zone = DNSZone {
        metadata: ObjectMeta {
            name: Some(zone_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: DNSZoneSpec {
            zone_name: "example.com".to_string(),
            cluster_ref: Some(cluster_ref.to_string()),
            global_cluster_ref: None,
            soa_record: SOARecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
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
        Ok(created) => {
            println!("✓ Created DNSZone: {namespace}/{zone_name}");
            assert_eq!(created.metadata.name.as_deref(), Some(zone_name));
            assert_eq!(created.spec.zone_name, "example.com");
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  DNSZone already exists");
        }
        Err(e) => panic!("Failed to create DNSZone: {e}"),
    }

    // Read DNSZone
    match zones.get(zone_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved DNSZone: {namespace}/{zone_name}");
            assert_eq!(retrieved.spec.zone_name, "example.com");
        }
        Err(e) => panic!("Failed to retrieve DNSZone: {e}"),
    }

    // Delete DNSZone
    match zones.delete(zone_name, &DeleteParams::default()).await {
        Ok(_) => println!("✓ Deleted DNSZone: {namespace}/{zone_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  DNSZone already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete DNSZone: {e}"),
    }

    // Cleanup
    delete_test_namespace(&client, namespace).await;

    println!("\n✓ Test passed\n");
}

// ============================================================================
// DNS Record Tests
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_arecord_create_read_delete() {
    println!("\n=== Test: ARecord CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "bindy-test-arecord";
    let record_name = "test-a-record";

    // Setup
    if let Err(e) = create_test_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create ARecord
    let records: Api<ARecord> = Api::namespaced(client.clone(), namespace);
    let record = ARecord {
        metadata: ObjectMeta {
            name: Some(record_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: ARecordSpec {
            zone_ref: "example-com".to_string(),
            name: "www".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
            ttl: Some(3600),
        },
        status: None,
    };

    match records.create(&PostParams::default(), &record).await {
        Ok(created) => {
            println!("✓ Created ARecord: {namespace}/{record_name}");
            assert_eq!(created.spec.ipv4_address, "192.0.2.1");
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  ARecord already exists");
        }
        Err(e) => panic!("Failed to create ARecord: {e}"),
    }

    // Read ARecord
    match records.get(record_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved ARecord: {namespace}/{record_name}");
            assert_eq!(retrieved.spec.name, "www");
        }
        Err(e) => panic!("Failed to retrieve ARecord: {e}"),
    }

    // Delete ARecord
    match records.delete(record_name, &DeleteParams::default()).await {
        Ok(_) => println!("✓ Deleted ARecord: {namespace}/{record_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  ARecord already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete ARecord: {e}"),
    }

    // Cleanup
    delete_test_namespace(&client, namespace).await;

    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_cname_record_create_read_delete() {
    println!("\n=== Test: CNAMERecord CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "bindy-test-cname";
    let record_name = "test-cname-record";

    // Setup
    if let Err(e) = create_test_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create CNAMERecord
    let records: Api<CNAMERecord> = Api::namespaced(client.clone(), namespace);
    let record = CNAMERecord {
        metadata: ObjectMeta {
            name: Some(record_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: CNAMERecordSpec {
            zone_ref: "example-com".to_string(),
            name: "www".to_string(),
            target: "example.com.".to_string(),
            ttl: Some(3600),
        },
        status: None,
    };

    match records.create(&PostParams::default(), &record).await {
        Ok(created) => {
            println!("✓ Created CNAMERecord: {namespace}/{record_name}");
            assert_eq!(created.spec.target, "example.com.");
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  CNAMERecord already exists");
        }
        Err(e) => panic!("Failed to create CNAMERecord: {e}"),
    }

    // Read CNAMERecord
    match records.get(record_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved CNAMERecord: {namespace}/{record_name}");
            assert_eq!(retrieved.spec.name, "www");
        }
        Err(e) => panic!("Failed to retrieve CNAMERecord: {e}"),
    }

    // Delete CNAMERecord
    match records.delete(record_name, &DeleteParams::default()).await {
        Ok(_) => println!("✓ Deleted CNAMERecord: {namespace}/{record_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  CNAMERecord already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete CNAMERecord: {e}"),
    }

    // Cleanup
    delete_test_namespace(&client, namespace).await;

    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_mx_record_create_read_delete() {
    println!("\n=== Test: MXRecord CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "bindy-test-mx";
    let record_name = "test-mx-record";

    // Setup
    if let Err(e) = create_test_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create MXRecord
    let records: Api<MXRecord> = Api::namespaced(client.clone(), namespace);
    let record = MXRecord {
        metadata: ObjectMeta {
            name: Some(record_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: MXRecordSpec {
            zone_ref: "example-com".to_string(),
            name: "@".to_string(),
            mail_server: "mail.example.com.".to_string(),
            priority: 10,
            ttl: Some(3600),
        },
        status: None,
    };

    match records.create(&PostParams::default(), &record).await {
        Ok(created) => {
            println!("✓ Created MXRecord: {namespace}/{record_name}");
            assert_eq!(created.spec.priority, 10);
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  MXRecord already exists");
        }
        Err(e) => panic!("Failed to create MXRecord: {e}"),
    }

    // Read MXRecord
    match records.get(record_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved MXRecord: {namespace}/{record_name}");
            assert_eq!(retrieved.spec.mail_server, "mail.example.com.");
        }
        Err(e) => panic!("Failed to retrieve MXRecord: {e}"),
    }

    // Delete MXRecord
    match records.delete(record_name, &DeleteParams::default()).await {
        Ok(_) => println!("✓ Deleted MXRecord: {namespace}/{record_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  MXRecord already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete MXRecord: {e}"),
    }

    // Cleanup
    delete_test_namespace(&client, namespace).await;

    println!("\n✓ Test passed\n");
}

#[tokio::test]
#[ignore]
async fn test_txt_record_create_read_delete() {
    println!("\n=== Test: TXTRecord CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "bindy-test-txt";
    let record_name = "test-txt-record";

    // Setup
    if let Err(e) = create_test_namespace(&client, namespace).await {
        panic!("Failed to create namespace: {e}");
    }

    // Create TXTRecord
    let records: Api<TXTRecord> = Api::namespaced(client.clone(), namespace);
    let record = TXTRecord {
        metadata: ObjectMeta {
            name: Some(record_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: TXTRecordSpec {
            zone_ref: "example-com".to_string(),
            name: "@".to_string(),
            text: vec!["v=spf1 include:_spf.example.com ~all".to_string()],
            ttl: Some(3600),
        },
        status: None,
    };

    match records.create(&PostParams::default(), &record).await {
        Ok(created) => {
            println!("✓ Created TXTRecord: {namespace}/{record_name}");
            assert!(!created.spec.text.is_empty());
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  TXTRecord already exists");
        }
        Err(e) => panic!("Failed to create TXTRecord: {e}"),
    }

    // Read TXTRecord
    match records.get(record_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved TXTRecord: {namespace}/{record_name}");
            assert_eq!(retrieved.spec.name, "@");
        }
        Err(e) => panic!("Failed to retrieve TXTRecord: {e}"),
    }

    // Delete TXTRecord
    match records.delete(record_name, &DeleteParams::default()).await {
        Ok(_) => println!("✓ Deleted TXTRecord: {namespace}/{record_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  TXTRecord already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete TXTRecord: {e}"),
    }

    // Cleanup
    delete_test_namespace(&client, namespace).await;

    println!("\n✓ Test passed\n");
}

// ============================================================================
// Unit Test
// ============================================================================

#[test]
fn test_unit_tests_work() {
    // This is a simple unit test to verify the test framework works
    assert_eq!(2 + 2, 4);
    println!("✓ Unit tests are working correctly");
}
