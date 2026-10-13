// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! CRD YAML Generator
//!
//! Generates Kubernetes CRD YAML files from Rust types defined in src/crd.rs.
//! This ensures the YAML files in deploy/operator/crds/ are always in sync with the Rust code.
//!
//! Usage:
//!   cargo run --bin crdgen
//!
//! Generated files will be written to deploy/operator/crds/ with proper headers.

use bindy_api::crd::{Bind9Cluster, Bind9Instance, ClusterBind9Provider, DNSZone};
use kube::{CustomResourceExt, Resource};
use serde_json::Value;
use std::fs;
use std::path::Path;

const COPYRIGHT_HEADER: &str = "# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# This file is AUTO-GENERATED from src/crd.rs
# DO NOT EDIT MANUALLY - Run `cargo run --bin crdgen` to regenerate
#
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = Path::new("deploy/operator/crds");

    // Ensure output directory exists
    fs::create_dir_all(output_dir)?;

    println!("Generating CRD YAML files from src/crd.rs...");

    // Every record kind, from the record-kind table (ADR-0020), then the
    // other CRDs. Each lands in `<plural>.crd.yaml`.
    bindy_api::each_record_kind!(K => generate_crd::<K>(output_dir)?);
    generate_crd::<DNSZone>(output_dir)?;
    generate_crd::<Bind9Cluster>(output_dir)?;
    generate_crd::<ClusterBind9Provider>(output_dir)?;
    generate_crd::<Bind9Instance>(output_dir)?;

    println!("✓ Successfully generated CRD YAML files in deploy/operator/crds/");
    println!("\nNext steps:");
    println!("  1. Review the generated files");
    println!("  2. Deploy with: kubectl apply --server-side -f deploy/operator/crds/");
    println!();
    println!("     --server-side is required: the Bind9Cluster / ClusterBind9Provider /");
    println!("     Bind9Instance CRDs exceed the 256KB last-applied-configuration");
    println!("     annotation limit, so a plain 'kubectl apply' is rejected.");

    Ok(())
}

/// Write `T`'s CRD to `<output_dir>/<plural>.crd.yaml`.
fn generate_crd<T>(output_dir: &Path) -> Result<(), Box<dyn std::error::Error>>
where
    T: CustomResourceExt + Resource<DynamicType = ()>,
{
    let filename = format!("{}.crd.yaml", T::plural(&()));
    let crd = T::crd();

    // Convert CRD to JSON for manipulation
    let mut crd_json: Value = serde_json::to_value(&crd)?;

    // Ensure v1beta1 is the storage version
    if let Some(versions) = crd_json["spec"]["versions"].as_array_mut() {
        if let Some(v1beta1_ref) = versions.first_mut() {
            v1beta1_ref["storage"] = Value::Bool(true);
            v1beta1_ref["served"] = Value::Bool(true);
        }
    }

    // Convert back to YAML
    let yaml = bindy_api::yaml::to_string(&crd_json)?;

    // Add copyright header
    let content = format!("{COPYRIGHT_HEADER}{yaml}");

    let output_path = output_dir.join(&filename);
    fs::write(&output_path, content)?;

    println!("  ✓ Generated {filename}");

    Ok(())
}
