// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! CRD Markdown Documentation Generator
//!
//! Generates markdown API reference documentation from Rust CRD types.
//! This ensures the documentation in docs/src/reference/api.md is always in sync with the code.
//!
//! Usage:
//!   cargo run --bin crddoc > docs/src/reference/api.md

use bindy::crd::{
    AAAARecord, ARecord, Bind9Cluster, Bind9Instance, CAARecord, CNAMERecord, DNSZone, MXRecord,
    NSRecord, SRVRecord, TXTRecord,
};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::JSONSchemaProps;
use kube::{CustomResourceExt, Resource};

fn main() {
    println!("# API Reference");
    println!();
    println!("This document describes the Custom Resource Definitions (CRDs) provided by Bindy.");
    println!();
    println!("> **Note**: This file is AUTO-GENERATED from `src/crd.rs`");
    println!("> DO NOT EDIT MANUALLY - Run `cargo run --bin crddoc` to regenerate");
    println!();

    // Table of Contents
    println!("## Table of Contents");
    println!();
    println!("- [Zone Management](#zone-management)");
    println!("  - [DNSZone](#dnszone)");
    println!("- [DNS Records](#dns-records)");
    println!("  - [ARecord](#arecord)");
    println!("  - [AAAARecord](#aaaarecord)");
    println!("  - [CNAMERecord](#cnamerecord)");
    println!("  - [MXRecord](#mxrecord)");
    println!("  - [NSRecord](#nsrecord)");
    println!("  - [TXTRecord](#txtrecord)");
    println!("  - [SRVRecord](#srvrecord)");
    println!("  - [CAARecord](#caarecord)");
    println!("- [Infrastructure](#infrastructure)");
    println!("  - [Bind9Cluster](#bind9cluster)");
    println!("  - [Bind9Instance](#bind9instance)");
    println!();

    // Zone Management
    println!("## Zone Management");
    println!();
    generate_crd_doc::<DNSZone>();

    // DNS Records
    println!("## DNS Records");
    println!();
    generate_crd_doc::<ARecord>();
    generate_crd_doc::<AAAARecord>();
    generate_crd_doc::<CNAMERecord>();
    generate_crd_doc::<MXRecord>();
    generate_crd_doc::<NSRecord>();
    generate_crd_doc::<TXTRecord>();
    generate_crd_doc::<SRVRecord>();
    generate_crd_doc::<CAARecord>();

    // Infrastructure
    println!("## Infrastructure");
    println!();
    generate_crd_doc::<Bind9Cluster>();
    generate_crd_doc::<Bind9Instance>();
}

fn generate_crd_doc<T>()
where
    T: CustomResourceExt + Resource<DynamicType = ()>,
{
    let crd = T::crd();
    let kind = T::kind(&());
    let group = T::group(&());
    let version = T::version(&());

    // Extract description from CRD
    let default_desc = format!("{kind} Custom Resource");
    let description = crd
        .spec
        .versions
        .first()
        .and_then(|v| v.schema.as_ref())
        .and_then(|s| s.open_api_v3_schema.as_ref())
        .and_then(|schema| schema.description.as_deref())
        .unwrap_or(&default_desc);

    println!("### {kind}");
    println!();
    println!("**API Version**: `{group}/{version}`");
    println!();
    println!("{description}");
    println!();

    // Extract spec schema
    if let Some(version_info) = crd.spec.versions.first() {
        if let Some(schema) = &version_info.schema {
            if let Some(open_api_schema) = &schema.open_api_v3_schema {
                if let Some(properties) = &open_api_schema.properties {
                    if let Some(spec_schema) = properties.get("spec") {
                        println!("#### Spec Fields");
                        println!();
                        print_schema_table(spec_schema, 0);
                        println!();
                    }

                    if let Some(status_schema) = properties.get("status") {
                        println!("#### Status Fields");
                        println!();
                        print_schema_table(status_schema, 0);
                        println!();
                    }
                }
            }
        }
    }

    println!("---");
    println!();
}

fn print_schema_table(schema: &JSONSchemaProps, _depth: usize) {
    if let Some(props) = &schema.properties {
        // Print table header
        println!("| Field | Type | Required | Description |");
        println!("| ----- | ---- | -------- | ----------- |");

        let required_fields = schema.required.clone().unwrap_or_default();

        // Sort properties for consistent output
        let mut sorted_props: Vec<_> = props.iter().collect();
        sorted_props.sort_by_key(|(name, _)| *name);

        for (name, prop_schema) in sorted_props {
            let is_required = required_fields.contains(name);
            let type_str = get_type_string(prop_schema);
            let description = get_description(prop_schema);

            let required_str = if is_required { "Yes" } else { "No" };

            println!("| `{name}` | {type_str} | {required_str} | {description} |");
        }
    }
}

fn get_type_string(schema: &JSONSchemaProps) -> String {
    // Check for $ref first (references to other types)
    if let Some(reference) = &schema.ref_path {
        // Extract type name from reference like "#/definitions/SOARecord"
        return reference
            .split('/')
            .next_back()
            .unwrap_or("object")
            .to_string();
    }

    // Check for type field
    if let Some(type_str) = &schema.type_ {
        if type_str == "array" {
            // Array type - items can be a schema or an array of schemas
            if schema.items.is_some() {
                // items is JSONSchemaPropsOrArray which can be Schema or SchemaArray
                // For simplicity, just return "array" for now
                // TODO: Extract item type if needed
                return "array".to_string();
            }
            return "array".to_string();
        }
        return type_str.clone();
    }

    // Check if it's an object with properties
    if schema.properties.is_some() {
        return "object".to_string();
    }

    "any".to_string()
}

fn get_description(schema: &JSONSchemaProps) -> String {
    if let Some(desc) = &schema.description {
        // Escape pipe characters in descriptions for markdown tables
        return desc.replace('|', "\\|").replace('\n', " ");
    }
    String::new()
}
