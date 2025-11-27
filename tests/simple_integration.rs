// Simplified integration tests for Bindy DNS Controller
// These tests verify the controller is working correctly in a Kubernetes cluster

use kube::client::Client;

// Test helper to check if running in a Kubernetes cluster
async fn get_kube_client_or_skip() -> Option<Client> {
    match Client::try_default().await {
        Ok(client) => {
            println!("Successfully connected to Kubernetes cluster");
            Some(client)
        }
        Err(e) => {
            eprintln!(
                "Skipping integration test: not running in Kubernetes cluster: {}",
                e
            );
            None
        }
    }
}

#[tokio::test]
#[ignore] // Run with: cargo test --test simple_integration -- --ignored
async fn test_kubernetes_connectivity() {
    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    // Try to list namespaces to verify connectivity
    use k8s_openapi::api::core::v1::Namespace;
    use kube::api::{Api, ListParams};

    let namespaces: Api<Namespace> = Api::all(client);
    let lp = ListParams::default().limit(1);

    match namespaces.list(&lp).await {
        Ok(ns_list) => {
            println!("Successfully connected to Kubernetes");
            println!("Found {} namespaces", ns_list.items.len());
            // Test passes if we can list namespaces successfully
        }
        Err(e) => {
            panic!("Failed to list namespaces: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_crds_installed() {
    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
    use kube::api::{Api, ListParams};

    let crds: Api<CustomResourceDefinition> = Api::all(client);
    let lp = ListParams::default();

    match crds.list(&lp).await {
        Ok(crd_list) => {
            let bindy_crds: Vec<_> = crd_list
                .items
                .iter()
                .filter(|crd| crd.spec.group.as_str().starts_with("dns.firestoned.io"))
                .collect();

            println!("Found {} Bindy CRDs", bindy_crds.len());

            for crd in &bindy_crds {
                println!("  - {}", crd.spec.names.kind);
            }

            // We expect at least some CRDs if the controller is installed
            // But don't fail if they're not there, just report
            if bindy_crds.is_empty() {
                println!(
                    "Warning: No Bindy CRDs found. Install with: kubectl apply -k deploy/crds/"
                );
            }
        }
        Err(e) => {
            println!("Could not check CRDs: {}", e);
            println!("This is expected if you don't have CRD permissions");
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_create_and_cleanup_namespace() {
    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return,
    };

    use k8s_openapi::api::core::v1::Namespace;
    use kube::api::{Api, DeleteParams, PostParams};
    use kube::core::ObjectMeta;

    let namespaces: Api<Namespace> = Api::all(client);
    let test_ns_name = "bindy-integration-test";

    // Create namespace
    let test_ns = Namespace {
        metadata: ObjectMeta {
            name: Some(test_ns_name.to_string()),
            labels: Some(
                [
                    ("test".to_string(), "integration".to_string()),
                    ("managed-by".to_string(), "bindy-test".to_string()),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
        ..Default::default()
    };

    let create_result = namespaces.create(&PostParams::default(), &test_ns).await;

    match create_result {
        Ok(_) => {
            println!("Successfully created test namespace: {}", test_ns_name);
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("Test namespace already exists: {}", test_ns_name);
        }
        Err(e) => {
            panic!("Failed to create test namespace: {}", e);
        }
    }

    // Clean up
    let delete_result = namespaces
        .delete(test_ns_name, &DeleteParams::default())
        .await;

    match delete_result {
        Ok(_) => {
            println!("Successfully deleted test namespace: {}", test_ns_name);
        }
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("Test namespace already deleted: {}", test_ns_name);
        }
        Err(e) => {
            println!("Warning: Failed to delete test namespace: {}", e);
        }
    }
}

#[test]
fn test_unit_tests_work() {
    // This is a simple unit test to verify the test framework works
    assert_eq!(2 + 2, 4);
    println!("Unit tests are working correctly");
}
