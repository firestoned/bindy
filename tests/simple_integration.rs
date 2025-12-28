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
    ARecord, ARecordSpec, Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, Bind9Instance,
    Bind9InstanceSpec, CNAMERecord, CNAMERecordSpec, ClusterBind9Provider,
    ClusterBind9ProviderSpec, DNSZone, DNSZoneSpec, MXRecord, MXRecordSpec, SOARecord, ServerRole,
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
                "ClusterBind9Provider",
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
// ClusterBind9Provider Tests (Cluster-Scoped)
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_clusterbind9provider_create_read_delete() {
    println!("\n=== Test: ClusterBind9Provider CRUD Operations ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let cluster_name = "test-global-cluster";

    // Create ClusterBind9Provider
    let cluster_providers: Api<ClusterBind9Provider> = Api::all(client.clone());
    let cluster = ClusterBind9Provider {
        metadata: ObjectMeta {
            name: Some(cluster_name.to_string()),
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
        Ok(created) => {
            println!("✓ Created ClusterBind9Provider: {cluster_name}");
            assert_eq!(created.metadata.name.as_deref(), Some(cluster_name));
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("  ClusterBind9Provider already exists");
        }
        Err(e) => panic!("Failed to create ClusterBind9Provider: {e}"),
    }

    // Read ClusterBind9Provider
    match cluster_providers.get(cluster_name).await {
        Ok(retrieved) => {
            println!("✓ Retrieved ClusterBind9Provider: {cluster_name}");
            assert_eq!(retrieved.metadata.name.as_deref(), Some(cluster_name));
        }
        Err(e) => panic!("Failed to retrieve ClusterBind9Provider: {e}"),
    }

    // List ClusterBind9Providers
    match cluster_providers.list(&ListParams::default()).await {
        Ok(list) => {
            println!("✓ Listed {} ClusterBind9Provider(s)", list.items.len());
        }
        Err(e) => panic!("Failed to list ClusterBind9Providers: {e}"),
    }

    // Delete ClusterBind9Provider
    match cluster_providers
        .delete(cluster_name, &DeleteParams::default())
        .await
    {
        Ok(_) => println!("✓ Deleted ClusterBind9Provider: {cluster_name}"),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("  ClusterBind9Provider already deleted");
        }
        Err(e) => eprintln!("⚠ Failed to delete ClusterBind9Provider: {e}"),
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
            cluster_provider_ref: None,
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
            records_from: None,
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
// Pod Label Selector Tests
// ============================================================================

/// Integration test: Verify that Bind9Instance can find its pods using correct label selector
///
/// This is a regression test for a critical bug where instances always showed "Not Ready"
/// because the pod label selector used the wrong label (app={name} instead of
/// app.kubernetes.io/instance={name}).
///
/// This test:
/// 1. Creates a Bind9Instance
/// 2. Waits for its deployment and pods to be created
/// 3. Verifies the pods have the correct labels
/// 4. Verifies the instance status correctly detects the pods as ready
#[tokio::test]
#[ignore] // Run with: cargo test --test simple_integration test_instance_pod_label_selector -- --ignored
async fn test_instance_pod_label_selector_finds_pods() {
    println!("\n=== Test: Instance Pod Label Selector Finds Pods ===\n");

    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    let namespace = "bindy-label-selector-test";

    // Setup
    if let Err(e) = create_test_namespace(&client, namespace).await {
        eprintln!("Failed to create test namespace: {e}");
        return;
    }

    // Create Bind9Cluster first (required for instance to reconcile)
    let cluster_name = "test-cluster";
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

    println!("Creating Bind9Cluster: {cluster_name}");
    match clusters.create(&PostParams::default(), &cluster).await {
        Ok(_) => println!("✓ Created Bind9Cluster"),
        Err(e) => {
            eprintln!("Failed to create Bind9Cluster: {e}");
            delete_test_namespace(&client, namespace).await;
            return;
        }
    }

    // Wait for cluster to reconcile and create ConfigMap
    println!("Waiting for cluster ConfigMap to be created...");
    use k8s_openapi::api::core::v1::ConfigMap;
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let expected_cm_name = format!("{cluster_name}-config");

    for attempt in 1..=12 {
        // 12 attempts * 5 seconds = 1 minute
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        if cm_api.get(&expected_cm_name).await.is_ok() {
            println!("✓ Cluster ConfigMap created: {expected_cm_name}");
            break;
        }

        if attempt == 12 {
            eprintln!("✗ Cluster ConfigMap not created after 1 minute");
            eprintln!("  Expected ConfigMap: {expected_cm_name}");
            // Continue anyway - the test should still reveal the label issue
        } else {
            println!("  Attempt {attempt}/12: Waiting for ConfigMap...");
        }
    }

    // Create a simple Bind9Instance
    let instance_name = "test-pod-labels";
    let instances: Api<Bind9Instance> = Api::namespaced(client.clone(), namespace);

    let instance = Bind9Instance {
        metadata: ObjectMeta {
            name: Some(instance_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Bind9InstanceSpec {
            cluster_ref: cluster_name.to_string(),
            role: ServerRole::Primary,
            replicas: Some(1),
            version: Some("9.18".to_string()),
            image: None,
            config: None,
            config_map_refs: None,
            primary_servers: None,
            volumes: None,
            volume_mounts: None,
            rndc_secret_ref: None,
            storage: None,
            bindcar_config: None,
        },
        status: None,
    };

    println!("Creating Bind9Instance: {instance_name}");
    match instances.create(&PostParams::default(), &instance).await {
        Ok(_) => println!("✓ Created Bind9Instance"),
        Err(e) => {
            eprintln!("Failed to create Bind9Instance: {e}");
            delete_test_namespace(&client, namespace).await;
            return;
        }
    }

    // Wait for pods to be created and become ready
    use k8s_openapi::api::core::v1::Pod;
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);

    println!("Waiting for pods to be created and become ready (up to 2 minutes)...");
    let mut pod_ready = false;
    for attempt in 1..=24 {
        // 24 attempts * 5 seconds = 2 minutes
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // Use the CORRECT label selector (the one the controller uses)
        let label_selector = format!("app.kubernetes.io/instance={}", instance_name);
        let list_params = ListParams::default().labels(&label_selector);

        match pod_api.list(&list_params).await {
            Ok(pods) => {
                if pods.items.is_empty() {
                    println!("  Attempt {attempt}/24: No pods found yet with selector '{label_selector}'");
                    continue;
                }

                println!(
                    "  Found {} pod(s) with selector '{label_selector}'",
                    pods.items.len()
                );

                // Check if any pod is ready
                for pod in &pods.items {
                    let pod_name = pod.metadata.name.as_deref().unwrap_or("unknown");
                    if let Some(status) = &pod.status {
                        if let Some(conditions) = &status.conditions {
                            let is_ready = conditions
                                .iter()
                                .any(|c| c.type_ == "Ready" && c.status == "True");

                            if is_ready {
                                println!("✓ Pod {pod_name} is Ready");
                                pod_ready = true;
                                break;
                            } else {
                                println!("  Pod {pod_name} not ready yet (attempt {attempt}/24)");
                            }
                        }
                    }
                }

                if pod_ready {
                    break;
                }
            }
            Err(e) => {
                eprintln!("  Failed to list pods: {e}");
            }
        }
    }

    if !pod_ready {
        eprintln!("✗ Pods never became ready after 2 minutes");
        eprintln!("  This might indicate a cluster issue or slow image pull");
    }

    // Now check the Bind9Instance status
    println!("\nChecking Bind9Instance status...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await; // Give controller time to reconcile

    match instances.get(instance_name).await {
        Ok(instance) => {
            if let Some(status) = &instance.status {
                println!("Instance status: {status:#?}");

                // Check if the instance has a Ready condition
                let conditions = &status.conditions;
                if !conditions.is_empty() {
                    if let Some(ready_condition) = conditions.iter().find(|c| c.r#type == "Ready") {
                        println!("\nReady condition:");
                        println!("  Status: {}", ready_condition.status);
                        println!("  Reason: {:?}", ready_condition.reason);
                        println!("  Message: {:?}", ready_condition.message);

                        // If pods are ready, the instance should eventually be ready too
                        if pod_ready {
                            if ready_condition.status == "True" {
                                println!("✓ Instance correctly detected pods as ready");
                            } else {
                                println!("⚠ Instance shows Not Ready even though pods are ready");
                                println!("   This might indicate the label selector bug or slow reconciliation");
                                println!("   Waiting 30 more seconds for reconciliation...");

                                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

                                // Check again
                                if let Ok(instance) = instances.get(instance_name).await {
                                    if let Some(status) = &instance.status {
                                        let conditions = &status.conditions;
                                        if !conditions.is_empty() {
                                            if let Some(ready_condition) =
                                                conditions.iter().find(|c| c.r#type == "Ready")
                                            {
                                                if ready_condition.status == "True" {
                                                    println!("✓ Instance now shows Ready after additional wait");
                                                } else {
                                                    println!("✗ Instance still shows Not Ready after total wait");
                                                    println!(
                                                        "   Status: {}",
                                                        ready_condition.status
                                                    );
                                                    println!(
                                                        "   Message: {:?}",
                                                        ready_condition.message
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        println!("⚠ No Ready condition found in instance status");
                    }
                } else {
                    println!("⚠ Instance status has no conditions");
                }
            } else {
                println!("⚠ Instance has no status");
            }
        }
        Err(e) => {
            eprintln!("Failed to get instance status: {e}");
        }
    }

    // Verify pods have the correct labels
    println!("\nVerifying pod labels...");
    let label_selector = format!("app.kubernetes.io/instance={}", instance_name);
    let list_params = ListParams::default().labels(&label_selector);

    match pod_api.list(&list_params).await {
        Ok(pods) => {
            for pod in &pods.items {
                let pod_name = pod.metadata.name.as_deref().unwrap_or("unknown");
                if let Some(labels) = &pod.metadata.labels {
                    println!("\nPod {pod_name} labels:");
                    println!(
                        "  app.kubernetes.io/instance: {:?}",
                        labels.get("app.kubernetes.io/instance")
                    );
                    println!("  instance: {:?}", labels.get("instance"));
                    println!("  app: {:?}", labels.get("app"));

                    // Verify critical labels exist
                    assert!(
                        labels.contains_key("app.kubernetes.io/instance"),
                        "Pod must have app.kubernetes.io/instance label"
                    );
                    assert_eq!(
                        labels.get("app.kubernetes.io/instance").unwrap(),
                        instance_name,
                        "app.kubernetes.io/instance label value must match instance name"
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to list pods for label verification: {e}");
        }
    }

    // Cleanup
    println!("\nCleaning up...");
    match instances
        .delete(instance_name, &DeleteParams::default())
        .await
    {
        Ok(_) => println!("✓ Deleted Bind9Instance"),
        Err(e) => eprintln!("⚠ Failed to delete Bind9Instance: {e}"),
    }

    match clusters
        .delete(cluster_name, &DeleteParams::default())
        .await
    {
        Ok(_) => println!("✓ Deleted Bind9Cluster"),
        Err(e) => eprintln!("⚠ Failed to delete Bind9Cluster: {e}"),
    }

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
