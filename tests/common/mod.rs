// Common test utilities for integration tests

use kube::{api::{Api, DeleteParams, PostParams}, client::Client};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Get a Kubernetes client or skip the test if not in a cluster
pub async fn get_kube_client_or_skip() -> Option<Client> {
    match Client::try_default().await {
        Ok(client) => Some(client),
        Err(e) => {
            eprintln!("Skipping integration test: not running in Kubernetes cluster: {}", e);
            None
        }
    }
}

/// Create a test namespace
pub async fn create_test_namespace(
    client: &Client,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let namespaces: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(client.clone());

    let ns = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {
            "name": name,
            "labels": {
                "test": "integration",
                "managed-by": "bindy-test"
            }
        }
    }))?;

    match namespaces.create(&PostParams::default(), &ns).await {
        Ok(_) => {
            println!("Created test namespace: {}", name);
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            println!("Test namespace already exists: {}", name);
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Cleanup test namespace
pub async fn cleanup_test_namespace(
    client: &Client,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let namespaces: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(client.clone());

    match namespaces.delete(name, &DeleteParams::default()).await {
        Ok(_) => {
            println!("Deleted test namespace: {}", name);
            Ok(())
        }
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            println!("Test namespace already deleted: {}", name);
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Create a Bind9Instance for testing
pub async fn create_bind9_instance(
    client: &Client,
    namespace: &str,
    name: &str,
    labels: Option<std::collections::HashMap<String, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let bind9_instances: Api<serde_json::Value> = Api::namespaced(client.clone(), namespace);

    let mut instance_labels = std::collections::HashMap::new();
    instance_labels.insert("bind9".to_string(), "instance".to_string());

    if let Some(extra_labels) = labels {
        instance_labels.extend(extra_labels);
    }

    let instance = json!({
        "apiVersion": "dns.example.com/v1alpha1",
        "kind": "Bind9Instance",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": instance_labels
        },
        "spec": {
            "replicas": 1,
            "version": "9.18",
            "config": {
                "recursion": false,
                "allowQuery": ["0.0.0.0/0"]
            }
        }
    });

    bind9_instances
        .create(&PostParams::default(), &serde_json::from_value(instance).unwrap())
        .await?;

    println!("Created Bind9Instance: {}/{}", namespace, name);
    Ok(())
}

/// Create a primary DNS zone for testing
pub async fn create_primary_zone(
    client: &Client,
    namespace: &str,
    zone_name: &str,
    domain: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let zones: Api<serde_json::Value> = Api::namespaced(client.clone(), namespace);

    let zone = json!({
        "apiVersion": "dns.example.com/v1alpha1",
        "kind": "DNSZone",
        "metadata": {
            "name": zone_name,
            "namespace": namespace
        },
        "spec": {
            "zoneName": domain,
            "zoneType": "primary",
            "instanceSelector": {
                "matchLabels": {
                    "bind9": "instance"
                }
            },
            "soaRecord": {
                "primaryNs": format!("ns1.{}", domain),
                "adminEmail": format!("admin@{}", domain.replace(".", "\\.")),
                "serial": 2024010101,
                "refresh": 3600,
                "retry": 600,
                "expire": 604800,
                "negativeTtl": 86400
            },
            "ttl": 3600
        }
    });

    zones
        .create(&PostParams::default(), &serde_json::from_value(zone).unwrap())
        .await?;

    println!("Created DNS zone: {}/{} ({})", namespace, zone_name, domain);
    Ok(())
}

/// Wait for a resource to be ready
pub async fn wait_for_ready(duration: Duration) {
    println!("Waiting {} seconds for resources to be ready...", duration.as_secs());
    sleep(duration).await;
}

/// Get the DNS service IP for a Bind9Instance
pub async fn get_dns_service_ip(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let services: Api<k8s_openapi::api::core::v1::Service> =
        Api::namespaced(client.clone(), namespace);

    match services.get(instance_name).await {
        Ok(svc) => {
            let ip = svc
                .spec
                .and_then(|s| s.cluster_ip)
                .unwrap_or_else(|| "10.0.0.1".to_string());
            Ok(ip)
        }
        Err(_) => Ok("10.0.0.1".to_string()),
    }
}

/// Query DNS using dig command (if available)
pub fn query_dns_with_dig(
    server: &str,
    domain: &str,
    record_type: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::process::Command;

    let output = Command::new("dig")
        .arg(format!("@{}", server))
        .arg(domain)
        .arg(record_type)
        .arg("+short")
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!(
            "dig command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

/// Setup complete DNS testing environment
pub async fn setup_dns_test_environment(
    namespace: &str,
) -> Result<Client, Box<dyn std::error::Error>> {
    let client = match get_kube_client_or_skip().await {
        Some(c) => c,
        None => return Err("Not in Kubernetes cluster".into()),
    };

    create_test_namespace(&client, namespace).await?;
    create_bind9_instance(&client, namespace, "test-dns", None).await?;
    create_primary_zone(&client, namespace, "test-zone", "test.example.com").await?;
    wait_for_ready(Duration::from_secs(10)).await;

    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_kube_client() {
        // This test will pass in cluster and skip outside
        match get_kube_client_or_skip().await {
            Some(_client) => {
                println!("Successfully connected to Kubernetes cluster");
            }
            None => {
                println!("Not in Kubernetes cluster - test skipped");
            }
        }
    }
}
