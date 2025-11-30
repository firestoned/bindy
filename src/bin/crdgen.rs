// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! CRD YAML Generator
//!
//! Generates Kubernetes CRD YAML files from Rust types defined in src/crd.rs.
//! This ensures the YAML files in deploy/crds/ are always in sync with the Rust code.
//!
//! Usage:
//!   cargo run --bin crdgen
//!
//! Generated files will be written to deploy/crds/ with proper headers.

use bindy::crd::{
    AAAARecord, ARecord, Bind9Cluster, Bind9Instance, CAARecord, CNAMERecord, DNSZone, MXRecord,
    NSRecord, SRVRecord, TXTRecord,
};
use kube::CustomResourceExt;
use std::fs;
use std::path::Path;

const COPYRIGHT_HEADER: &str = "# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# This file is AUTO-GENERATED from src/crd.rs
# DO NOT EDIT MANUALLY - Run `cargo run --bin crdgen` to regenerate
#
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = Path::new("deploy/crds");

    // Ensure output directory exists
    fs::create_dir_all(output_dir)?;

    println!("Generating CRD YAML files from src/crd.rs...");

    // Generate each CRD
    generate_crd::<ARecord>("arecords.crd.yaml", output_dir)?;
    generate_crd::<AAAARecord>("aaaarecords.crd.yaml", output_dir)?;
    generate_crd::<CNAMERecord>("cnamerecords.crd.yaml", output_dir)?;
    generate_crd::<MXRecord>("mxrecords.crd.yaml", output_dir)?;
    generate_crd::<NSRecord>("nsrecords.crd.yaml", output_dir)?;
    generate_crd::<TXTRecord>("txtrecords.crd.yaml", output_dir)?;
    generate_crd::<SRVRecord>("srvrecords.crd.yaml", output_dir)?;
    generate_crd::<CAARecord>("caarecords.crd.yaml", output_dir)?;
    generate_crd::<DNSZone>("dnszones.crd.yaml", output_dir)?;
    generate_crd::<Bind9Cluster>("bind9clusters.crd.yaml", output_dir)?;
    generate_crd::<Bind9Instance>("bind9instances.crd.yaml", output_dir)?;

    println!("✓ Successfully generated CRD YAML files in deploy/crds/");
    println!("\nNext steps:");
    println!("  1. Review the generated files");
    println!("  2. Deploy with: kubectl apply -f deploy/crds/");

    Ok(())
}

fn generate_crd<T>(filename: &str, output_dir: &Path) -> Result<(), Box<dyn std::error::Error>>
where
    T: CustomResourceExt,
{
    let crd = T::crd();
    let yaml = serde_yaml::to_string(&crd)?;

    // Add copyright header
    let content = format!("{COPYRIGHT_HEADER}{yaml}");

    let output_path = output_dir.join(filename);
    fs::write(&output_path, content)?;

    println!("  ✓ Generated {filename}");

    Ok(())
}
