// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! `named.conf` and `named.conf.options` rendering (ADR-0013), and the
//! `dnssec-policy` statement bindy defines.

use super::dnssec::{dnssec_key_secret, get_dnssec_signing_config, DEFAULT_DNSSEC_POLICY_NAME};
use super::{
    BIND_CACHE_PATH, BIND_DNSSEC_KEYS_PATH, BIND_KEYS_PATH, BIND_NAMED_CONF_OPTIONS_PATH,
    BIND_NAMED_CONF_ZONES_PATH,
};
use crate::bind9_acl::parse_acl_list;
use crate::constants::{DNS_CONTAINER_PORT, RNDC_PORT};
use crate::crd::{Bind9Config, ConfigMapRefs};
use anyhow::Context;
use hornet_bind9::named_conf::{
    AddressMatchElement, AddressMatchList, ControlsBlock, DnssecKeyLifetime, DnssecKeyRole,
    DnssecPolicyKey, DnssecPolicyStmt, DnssecValidation, InetControl, ListenOn, LogCategory,
    LogChannel, LogDestination, LogSeverity, LoggingBlock, NamedConf, Nsec3Param, OptionsBlock,
    PrintTime, RateLimit, Statement,
};
use hornet_bind9::writer::WriteOptions;
use std::net::{IpAddr, Ipv4Addr};

/// Name of the RNDC key `named` accepts on its control channel. The key file
/// ([`RNDC_KEY_FILENAME`] in [`BIND_KEYS_PATH`]) is mounted from the RNDC Secret.
const RNDC_KEY_NAME: &str = "bindy-operator";
/// File name of the RNDC key inside [`BIND_KEYS_PATH`].
const RNDC_KEY_FILENAME: &str = "rndc.key";

// Files `named` writes in its working directory (`BIND_CACHE_PATH`).
const NAMED_PID_FILENAME: &str = "named.pid";
const NAMED_SESSION_KEY_FILENAME: &str = "session.key";
const NAMED_DUMP_FILENAME: &str = "cache_dump.db";
const NAMED_STATISTICS_FILENAME: &str = "named_stats.txt";
const NAMED_MEMSTATISTICS_FILENAME: &str = "named_mem_stats.txt";

// Logging: every channel goes to stderr, where the kubelet collects it.
const LOG_CHANNEL_DEFAULT: &str = "default_stderr";
const LOG_CHANNEL_QUERIES: &str = "queries_stderr";
const LOG_CHANNEL_SECURITY: &str = "security_stderr";
/// Number of logging categories bindy routes.
const LOG_CATEGORY_COUNT: usize = 12;
/// Each logging category and the channel it is routed to.
const LOG_CATEGORIES: [(&str, &str); LOG_CATEGORY_COUNT] = [
    ("default", LOG_CHANNEL_DEFAULT),
    ("general", LOG_CHANNEL_DEFAULT),
    ("config", LOG_CHANNEL_DEFAULT),
    ("network", LOG_CHANNEL_DEFAULT),
    ("queries", LOG_CHANNEL_QUERIES),
    ("security", LOG_CHANNEL_SECURITY),
    ("dnssec", LOG_CHANNEL_SECURITY),
    ("xfer-in", LOG_CHANNEL_DEFAULT),
    ("xfer-out", LOG_CHANNEL_DEFAULT),
    ("notify", LOG_CHANNEL_DEFAULT),
    ("update", LOG_CHANNEL_DEFAULT),
    ("update-security", LOG_CHANNEL_SECURITY),
];

// Timing clauses of the DNSSEC policy bindy defines.
const DNSSEC_SIGNATURES_REFRESH: &str = "5d";
const DNSSEC_SIGNATURES_VALIDITY: &str = "30d";
const DNSSEC_SIGNATURES_VALIDITY_DNSKEY: &str = "30d";
/// Time for zone updates to reach all servers: 5 minutes, in seconds.
const DNSSEC_ZONE_PROPAGATION_DELAY: &str = "300";
/// Time for DS updates to reach the parent zone: 1 hour, in seconds.
const DNSSEC_PARENT_PROPAGATION_DELAY: &str = "3600";
/// Maximum TTL in a signed zone, which bounds key rollover timing: 24 hours,
/// in seconds.
const DNSSEC_MAX_ZONE_TTL: &str = "86400";

/// BIND's keyword for a key that never rolls; the only lifetime shared keys
/// may have (ADR-0012), and the default KSK and ZSK lifetime.
const DNSSEC_LIFETIME_UNLIMITED: &str = "unlimited";

// named.conf.options directive names for listen addresses
const LISTEN_ON_DIRECTIVE: &str = "listen-on";
const LISTEN_ON_V6_DIRECTIVE: &str = "listen-on-v6";

// Default DNSSEC signing parameters

/// Policy names BIND reserves for its built-in policies. A
/// `dnssec-policy "<name>" { ... }` block with one of these makes named refuse
/// to load its configuration ("dnssec-policy name may not be 'insecure',
/// 'none', or 'default'"), so a definition may never use them.
const BIND_BUILTIN_DNSSEC_POLICIES: [&str; 3] = ["default", "insecure", "none"];
const DEFAULT_DNSSEC_ALGORITHM: &str = "ECDSAP256SHA256";
const DEFAULT_NSEC3_SALT_LENGTH: u8 = 16;

/// Max length of a DNSSEC policy NAME, matching the CRD schema pattern
/// `^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$` and ValidatingAdmissionPolicy 09.
const MAX_DNSSEC_POLICY_NAME_LEN: usize = 63;

/// Max length of a DNSSEC signing TOKEN (algorithm, KSK/ZSK lifetime), matching
/// the CRD schema pattern `^[A-Za-z0-9]{1,32}$` and ValidatingAdmissionPolicy 09.
const MAX_DNSSEC_TOKEN_LEN: usize = 32;

/// Validate a DNSSEC policy name against `^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$`,
/// and refuse BIND's built-in policy names (`default`, `insecure`, `none`).
///
/// This is the RUNTIME arm of a three-layer defence. The CRD schema rejects a
/// bad value at the API server and ValidatingAdmissionPolicy 09 rejects it at
/// admission — but a cluster running a stale CRD, or one that never installed
/// the policy suite, would otherwise interpolate the value straight into the
/// `dnssec-policy { ... }` block of `named.conf` (audit finding P2-5). All three
/// layers deliberately share one grammar so they cannot disagree.
///
/// # Errors
/// Returns an error naming the field when `name` does not match the grammar.
fn validate_dnssec_policy_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.len() > MAX_DNSSEC_POLICY_NAME_LEN {
        anyhow::bail!(
            "invalid dnssec policy name {name:?}: must be 1-{MAX_DNSSEC_POLICY_NAME_LEN} characters"
        );
    }

    let mut chars = name.chars();
    // Guard clause: the first character may not be '-' or '_'.
    let first = chars.next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        anyhow::bail!(
            "invalid dnssec policy name {name:?}: must start with an ASCII letter or digit"
        );
    }
    if let Some(bad) = chars.find(|c| !c.is_ascii_alphanumeric() && *c != '-' && *c != '_') {
        anyhow::bail!(
            "invalid dnssec policy name {name:?}: illegal character {bad:?} \
             (allowed: ASCII letters, digits, '-', '_')"
        );
    }
    if BIND_BUILTIN_DNSSEC_POLICIES
        .iter()
        .any(|builtin| name.eq_ignore_ascii_case(builtin))
    {
        anyhow::bail!(
            "invalid dnssec policy name {name:?}: it is a BIND built-in policy, which \
             named refuses to redefine; choose another name (e.g. {DEFAULT_DNSSEC_POLICY_NAME:?})"
        );
    }

    Ok(())
}

/// Validate a DNSSEC signing token (algorithm, KSK/ZSK lifetime) against
/// `^[A-Za-z0-9]{1,32}$`.
///
/// These are interpolated UNQUOTED into `named.conf`, so the grammar is stricter
/// than for policy names: alphanumeric only. `field` names the offending input in
/// the error so an operator can find it without reading the template.
///
/// # Errors
/// Returns an error naming `field` when `value` does not match the grammar.
fn validate_dnssec_token(field: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty() || value.len() > MAX_DNSSEC_TOKEN_LEN {
        anyhow::bail!(
            "invalid {field} {value:?}: must be 1-{MAX_DNSSEC_TOKEN_LEN} alphanumeric characters"
        );
    }
    if let Some(bad) = value.chars().find(|c| !c.is_ascii_alphanumeric()) {
        anyhow::bail!(
            "invalid {field} {value:?}: illegal character {bad:?} (allowed: ASCII letters and digits)"
        );
    }

    Ok(())
}

/// The `dnssec-policy` statement bindy defines, from cluster or instance config
///
/// Checks both instance and global configuration for DNSSEC signing settings.
/// Instance config takes precedence over global config.
///
/// # Arguments
///
/// * `global_config` - Optional global cluster configuration
/// * `instance_config` - Optional instance-specific configuration
///
/// # Returns
///
/// The policy statement, or `None` if signing is not enabled anywhere (which
/// is not an error).
///
/// # Errors
///
/// Returns an error if `policy`, `algorithm`, `kskLifetime` or `zskLifetime`
/// fails the runtime whitelist (see [`validate_dnssec_policy_name`] and
/// [`validate_dnssec_token`], audit finding P2-5), or if a lifetime is not a
/// duration BIND accepts (see [`policy_key_lifetime`]).
pub(crate) fn dnssec_policy_statement(
    global_config: Option<&crate::crd::Bind9Config>,
    instance_config: Option<&crate::crd::Bind9Config>,
) -> anyhow::Result<Option<DnssecPolicyStmt>> {
    // Resolve the signing config with the same precedence/fallback semantics
    // as get_dnssec_signing_config: instance config wins when it enables
    // signing, otherwise fall back to the global config.
    let Some(signing) = get_dnssec_signing_config(global_config, instance_config) else {
        return Ok(None);
    };

    // Extract policy parameters with defaults
    let policy_name = signing
        .policy
        .as_deref()
        .unwrap_or(DEFAULT_DNSSEC_POLICY_NAME);
    let algorithm = signing
        .algorithm
        .as_deref()
        .unwrap_or(DEFAULT_DNSSEC_ALGORITHM);
    let ksk_lifetime = signing
        .ksk_lifetime
        .as_deref()
        .unwrap_or(DNSSEC_LIFETIME_UNLIMITED);
    let zsk_lifetime = signing
        .zsk_lifetime
        .as_deref()
        .unwrap_or(DNSSEC_LIFETIME_UNLIMITED);

    // P2-5: the algorithm and lifetimes are written unquoted. hornet's writer
    // emits them as given, so they are still held to the CRD's grammar here.
    validate_dnssec_policy_name(policy_name)?;
    validate_dnssec_token("dnssec algorithm", algorithm)?;
    validate_dnssec_token("dnssec ksk lifetime", ksk_lifetime)?;
    validate_dnssec_token("dnssec zsk lifetime", zsk_lifetime)?;
    if dnssec_key_secret(signing).is_some() {
        require_unlimited_lifetime("kskLifetime", ksk_lifetime)?;
        require_unlimited_lifetime("zskLifetime", zsk_lifetime)?;
    }

    let key = |role, lifetime| DnssecPolicyKey {
        role,
        storage: None,
        lifetime,
        algorithm: algorithm.to_string(),
        tag_range: None,
        bits: None,
    };
    let keys = vec![
        key(
            DnssecKeyRole::Ksk,
            policy_key_lifetime("kskLifetime", ksk_lifetime)?,
        ),
        key(
            DnssecKeyRole::Zsk,
            policy_key_lifetime("zskLifetime", zsk_lifetime)?,
        ),
    ];

    // NSEC is selected by having no `nsec3param` clause.
    let nsec3param = signing.nsec3.unwrap_or(false).then(|| Nsec3Param {
        iterations: Some(signing.nsec3_iterations.unwrap_or(0)),
        optout: Some(false),
        salt_length: Some(u32::from(DEFAULT_NSEC3_SALT_LENGTH)),
    });

    Ok(Some(DnssecPolicyStmt {
        name: policy_name.to_string(),
        keys: Some(keys),
        nsec3param,
        signatures_refresh: Some(DNSSEC_SIGNATURES_REFRESH.to_string()),
        signatures_validity: Some(DNSSEC_SIGNATURES_VALIDITY.to_string()),
        signatures_validity_dnskey: Some(DNSSEC_SIGNATURES_VALIDITY_DNSKEY.to_string()),
        zone_propagation_delay: Some(DNSSEC_ZONE_PROPAGATION_DELAY.to_string()),
        parent_propagation_delay: Some(DNSSEC_PARENT_PROPAGATION_DELAY.to_string()),
        max_zone_ttl: Some(DNSSEC_MAX_ZONE_TTL.to_string()),
        ..Default::default()
    }))
}

/// A policy key lifetime: `unlimited`, or a duration BIND accepts.
///
/// BIND has no `y` TTL unit, so `1y` makes `named` refuse the whole
/// configuration; one year is `P1Y`, `365d` or `8760h`.
///
/// # Errors
/// Returns an error naming `field` when `value` is neither `unlimited` nor a
/// BIND duration.
fn policy_key_lifetime(field: &str, value: &str) -> anyhow::Result<DnssecKeyLifetime> {
    if value.eq_ignore_ascii_case(DNSSEC_LIFETIME_UNLIMITED) {
        return Ok(DnssecKeyLifetime::Unlimited);
    }
    if !hornet_bind9::named_conf::is_duration(value) {
        anyhow::bail!(
            "invalid dnssec {field} {value:?}: not a duration BIND accepts; use a TTL value \
             such as 365d or 8760h, an ISO 8601 duration such as P1Y, or \
             {DNSSEC_LIFETIME_UNLIMITED:?}"
        );
    }
    Ok(DnssecKeyLifetime::Duration(value.to_string()))
}

/// Refuse a finite key lifetime for keys supplied from a Secret (ADR-0012).
///
/// The keys are shared by every primary; a finite lifetime makes each `named`
/// roll a successor of its own, and the pods diverge. The CRD rejects this at
/// admission; this is the runtime arm for a cluster with an older CRD.
///
/// # Errors
/// Returns an error naming `field` when `value` is not `unlimited`.
fn require_unlimited_lifetime(field: &str, value: &str) -> anyhow::Result<()> {
    if value.eq_ignore_ascii_case(DNSSEC_LIFETIME_UNLIMITED) {
        return Ok(());
    }
    anyhow::bail!(
        "invalid dnssec {field} {value:?}: keys from keysFrom.secretRef are shared by every \
         primary and must not roll on their own; use {DNSSEC_LIFETIME_UNLIMITED:?} (or leave \
         it unset) and rotate by updating the Secret"
    )
}

/// The `key-directory` for `named.conf.options`: the DNSSEC key mount
/// ([`BIND_DNSSEC_KEYS_PATH`]) when signing is enabled, else none.
///
/// Without it BIND keeps `dnssec-policy` keys in its working directory, a
/// scratch volume, so keys from `keysFrom` are never read and generated keys
/// never reach the volume that is meant to keep them.
///
/// # Arguments
///
/// * `global_config` - Optional global cluster configuration
/// * `instance_config` - Optional instance-specific configuration
fn key_directory(
    global_config: Option<&crate::crd::Bind9Config>,
    instance_config: Option<&crate::crd::Bind9Config>,
) -> Option<String> {
    get_dnssec_signing_config(global_config, instance_config)
        .map(|_| BIND_DNSSEC_KEYS_PATH.to_string())
}

/// Write statements as BIND9 configuration text with hornet's writer
/// (ADR-0013 stage 3): every value is quoted or escaped for its position by
/// the writer, not by each caller.
fn write_conf(statements: Vec<Statement>) -> String {
    hornet_bind9::write_named_conf(&NamedConf { statements }, &WriteOptions::default())
}

/// Build the main named.conf, for an instance or a cluster.
///
/// Includes `named.conf.options`, the zones file when the user provides a
/// `namedConfZones` `ConfigMap`, and the RNDC key; then the control channel
/// and logging.
///
/// # Arguments
///
/// * `config_map_refs` - The custom `ConfigMap` references in effect: an
///   instance's own, else its cluster's; a cluster's own for the shared
///   `ConfigMap`
///
/// # Returns
///
/// A string containing the complete named.conf configuration
pub(super) fn build_named_conf(config_map_refs: Option<&ConfigMapRefs>) -> String {
    render_named_conf(includes_zones_file(config_map_refs))
}

/// Whether the user supplies a `namedConfZones` `ConfigMap` to include.
fn includes_zones_file(config_map_refs: Option<&ConfigMapRefs>) -> bool {
    config_map_refs.is_some_and(|refs| refs.named_conf_zones.is_some())
}

/// Render named.conf.
///
/// # Arguments
///
/// * `include_zones` - Include the user-provided zones file
fn render_named_conf(include_zones: bool) -> String {
    let mut statements = vec![Statement::Include(BIND_NAMED_CONF_OPTIONS_PATH.to_string())];
    if include_zones {
        statements.push(Statement::Include(BIND_NAMED_CONF_ZONES_PATH.to_string()));
    }
    statements.push(Statement::Include(format!(
        "{BIND_KEYS_PATH}/{RNDC_KEY_FILENAME}"
    )));
    statements.push(Statement::Controls(rndc_controls()));
    statements.push(Statement::Logging(logging()));
    write_conf(statements)
}

/// The RNDC control channel: localhost only, with the operator's key. The
/// bindcar sidecar handles access from outside the pod.
fn rndc_controls() -> ControlsBlock {
    ControlsBlock {
        inet: vec![InetControl {
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: RNDC_PORT,
            allow: vec![AddressMatchElement::Localhost],
            keys: vec![RNDC_KEY_NAME.to_string()],
            read_only: None,
        }],
        unix: Vec::new(),
    }
}

/// Logging: three stderr channels, ISO 8601 timestamps, and the
/// [`LOG_CATEGORIES`] routing.
fn logging() -> LoggingBlock {
    let channel = |name: &str, severity| LogChannel {
        name: name.to_string(),
        destination: LogDestination::Stderr,
        severity: Some(severity),
        print_time: Some(PrintTime::Iso8601),
        print_severity: Some(true),
        print_category: Some(true),
        buffered: None,
    };
    LoggingBlock {
        channels: vec![
            channel(LOG_CHANNEL_DEFAULT, LogSeverity::Info),
            channel(LOG_CHANNEL_QUERIES, LogSeverity::Info),
            channel(LOG_CHANNEL_SECURITY, LogSeverity::Warning),
        ],
        categories: LOG_CATEGORIES
            .iter()
            .map(|(name, channel)| LogCategory {
                name: (*name).to_string(),
                channels: vec![(*channel).to_string()],
            })
            .collect(),
    }
}

/// The `options` fields every rendered configuration has: `named`'s working
/// files under [`BIND_CACHE_PATH`], and `allow-new-zones` for bindcar's
/// `rndc addzone`.
fn base_options() -> OptionsBlock {
    OptionsBlock {
        directory: Some(BIND_CACHE_PATH.to_string()),
        pid_file: Some(format!("{BIND_CACHE_PATH}/{NAMED_PID_FILENAME}")),
        session_keyfile: Some(format!("{BIND_CACHE_PATH}/{NAMED_SESSION_KEY_FILENAME}")),
        dump_file: Some(format!("{BIND_CACHE_PATH}/{NAMED_DUMP_FILENAME}")),
        statistics_file: Some(format!("{BIND_CACHE_PATH}/{NAMED_STATISTICS_FILENAME}")),
        memstatistics_file: Some(format!("{BIND_CACHE_PATH}/{NAMED_MEMSTATISTICS_FILENAME}")),
        allow_new_zones: Some(true),
        ..Default::default()
    }
}

/// Render named.conf.options: the `options` block, then the DNSSEC policy it
/// signs with, if any.
fn render_options_conf(options: OptionsBlock, policy: Option<DnssecPolicyStmt>) -> String {
    let mut statements = vec![Statement::Options(options)];
    statements.extend(policy.map(Statement::DnssecPolicy));
    write_conf(statements)
}

/// `allow-transfer { none; }`: emitted at the options level when no explicit
/// transfer ACL is configured anywhere (no instance, role, or global
/// `allow_transfer`).
///
/// BIND9's built-in default is `allow-transfer { any; }`, which would expose
/// every zone served by the instance to AXFR from any client: bulk zone
/// enumeration (threat model I2) and an amplification vector (D3). We deny by
/// default instead. Zones that legitimately need transfers get a **zone-level**
/// `allow-transfer` ACL scoped to their secondary IPs (see
/// `bind9::zone_ops`), and a zone-level ACL overrides this options-level
/// default in BIND9, so replication is unaffected by this hardening.
fn deny_all_transfers() -> AddressMatchList {
    vec![AddressMatchElement::None]
}

/// Default `responses-per-second` for BIND9 Response Rate Limiting (RRL) when
/// `spec.config.rateLimit` is not set. RRL is on by default (threat model
/// D1/D3, DNS amplification/reflection); a per-source-prefix cap of 15/s is
/// ISC's recommended conservative starting point and rarely affects legitimate
/// clients. Set `rateLimit.responsesPerSecond: 0` in the CRD to disable.
const DEFAULT_RATE_LIMIT_RESPONSES_PER_SECOND: u32 = 15;

/// Error-context name for the cluster-level `allow_query` field.
const SOURCE_GLOBAL_ALLOW_QUERY: &str = "cluster spec.global.allow_query";

/// Error-context name for the cluster-level `allow_transfer` field.
const SOURCE_GLOBAL_ALLOW_TRANSFER: &str = "cluster spec.global.allow_transfer";

/// Build the named.conf.options configuration, for an instance or a cluster.
///
/// Generates the BIND9 options configuration file from the config spec.
/// Includes settings for recursion, ACLs (allow-query, allow-transfer), DNSSEC,
/// forwarders, and listen addresses (listen-on / listen-on-v6).
///
/// Priority for configuration values (highest to lowest):
/// 1. Instance-level settings (`instance.spec.config`)
/// 2. Role-specific settings (`role_allow_transfer` from cluster primary/secondary spec)
/// 3. Global cluster settings (`cluster.spec.common.global`)
/// 4. Defaults (BIND9 defaults or no setting)
///
/// A cluster's shared `ConfigMap` passes no instance config and no role ACL,
/// so it renders exactly what an instance that sets neither renders.
///
/// # Arguments
///
/// * `instance_cfg` - The instance's `spec.config`; `None` for a cluster
/// * `global_config` - The cluster's `spec.common.global`, if any
/// * `role_allow_transfer` - Role-specific allow-transfer override from cluster
///   spec (primary/secondary); `None` for a cluster
///
/// # Returns
///
/// A string containing the complete named.conf.options configuration
///
/// # Errors
///
/// Returns an error if an ACL, forwarder, listen address or DNSSEC signing
/// value fails validation.
pub(super) fn build_options_conf(
    instance_cfg: Option<&Bind9Config>,
    global_config: Option<&Bind9Config>,
    role_allow_transfer: Option<&Vec<String>>,
) -> anyhow::Result<String> {
    // Allow-query ACL: the first configured level wins (instance, then
    // global); an explicitly empty list renders no directive.
    let allow_query = if let Some(acls) = instance_cfg.and_then(|c| c.allow_query.as_ref()) {
        acl_list(Some(acls), "instance spec.config.allow_query")?
    } else {
        acl_list(
            global_config.and_then(|g| g.allow_query.as_ref()),
            SOURCE_GLOBAL_ALLOW_QUERY,
        )?
    };

    // Allow-transfer ACL: priority instance config > role-specific > global.
    // An explicitly empty list at any level means `none`; with no explicit ACL
    // anywhere, deny by default (see `deny_all_transfers`).
    let allow_transfer = if let Some(acls) = instance_cfg.and_then(|c| c.allow_transfer.as_ref()) {
        transfer_acl(acls, "instance spec.config.allow_transfer")?
    } else if let Some(role_acls) = role_allow_transfer {
        transfer_acl(role_acls, "cluster role-specific allow_transfer")?
    } else if let Some(global_acls) = global_config.and_then(|g| g.allow_transfer.as_ref()) {
        transfer_acl(global_acls, SOURCE_GLOBAL_ALLOW_TRANSFER)?
    } else {
        deny_all_transfers()
    };

    // Every other option: instance overrides global, per field.
    let options = OptionsBlock {
        listen_on: vec![listen_on(
            LISTEN_ON_DIRECTIVE,
            instance_cfg
                .and_then(|c| c.listen_on.as_ref())
                .or_else(|| global_config.and_then(|g| g.listen_on.as_ref())),
        )?],
        listen_on_v6: vec![listen_on(
            LISTEN_ON_V6_DIRECTIVE,
            instance_cfg
                .and_then(|c| c.listen_on_v6.as_ref())
                .or_else(|| global_config.and_then(|g| g.listen_on_v6.as_ref())),
        )?],
        // Recursion is off when neither level sets it.
        recursion: Some(
            instance_cfg
                .and_then(|c| c.recursion)
                .or_else(|| global_config.and_then(|g| g.recursion))
                .unwrap_or(false),
        ),
        forwarders: forwarders(
            instance_cfg
                .and_then(|c| c.forwarders.as_ref())
                .or_else(|| global_config.and_then(|g| g.forwarders.as_ref())),
        )?,
        allow_query,
        allow_transfer: Some(allow_transfer),
        // Response Rate Limiting is on by default.
        rate_limit: rate_limit(
            instance_cfg
                .and_then(|c| c.rate_limit.as_ref())
                .or_else(|| global_config.and_then(|g| g.rate_limit.as_ref())),
        ),
        // dnssec-enable was removed in BIND 9.15+ (DNSSEC is always enabled);
        // only validation is configurable.
        dnssec_validation: resolve_dnssec_validation(instance_cfg, global_config),
        key_directory: key_directory(global_config, instance_cfg),
        ..base_options()
    };

    Ok(render_options_conf(
        options,
        dnssec_policy_statement(global_config, instance_cfg)?,
    ))
}

/// The `forwarders` list for named.conf.options.
///
/// Emits only the `forwarders` block (no `forward` mode statement), matching
/// BIND defaults. Empty when `forwarders` is `None` or empty, so no directive
/// is rendered.
///
/// # Arguments
///
/// * `forwarders` - Optional list of upstream DNS server IP addresses
///
/// # Errors
///
/// Returns an error if any entry is not a plain IPv4 or IPv6 address.
fn forwarders(forwarders: Option<&Vec<String>>) -> anyhow::Result<Vec<IpAddr>> {
    forwarders
        .into_iter()
        .flatten()
        .map(|entry| {
            let trimmed = entry.trim();
            trimmed.parse().map_err(|_| {
                anyhow::anyhow!(
                    "invalid forwarder {trimmed:?}: must be a plain IPv4 or IPv6 address"
                )
            })
        })
        .collect()
}

/// The `rate-limit { responses-per-second N; }` block for named.conf.options.
///
/// Response Rate Limiting (RRL) is **on by default**: when `rate_limit` is
/// `None`, or its `responses_per_second` is `None`, the conservative default
/// [`DEFAULT_RATE_LIMIT_RESPONSES_PER_SECOND`] is used. An explicit value of
/// `0` disables RRL: no block is emitted.
///
/// # Arguments
///
/// * `rate_limit` - Optional RRL config (instance value takes priority over the
///   cluster `global` value; resolve that before calling).
fn rate_limit(rate_limit: Option<&crate::crd::RateLimitConfig>) -> Option<RateLimit> {
    let rps = rate_limit
        .and_then(|r| r.responses_per_second)
        .unwrap_or(DEFAULT_RATE_LIMIT_RESPONSES_PER_SECOND);
    if rps == 0 {
        return None;
    }
    Some(RateLimit {
        responses_per_second: Some(rps),
        ..Default::default()
    })
}

/// A `listen-on` / `listen-on-v6` directive for named.conf.options.
///
/// Defaults to `{ any; }` when `addresses` is `None` or empty. The port is
/// [`DNS_CONTAINER_PORT`] (the unprivileged port `named` binds inside the pod;
/// the DNS Service exposes [`DNS_PORT`](crate::constants::DNS_PORT) to clients and forwards to it), not
/// the client-facing port.
///
/// # Arguments
///
/// * `directive` - Either [`LISTEN_ON_DIRECTIVE`] or [`LISTEN_ON_V6_DIRECTIVE`],
///   for error context
/// * `addresses` - Optional address match list from the CRD
///
/// # Errors
///
/// Returns an error if any entry fails address-match-list validation; see
/// [`crate::bind9_acl`] for the accepted syntax.
fn listen_on(directive: &str, addresses: Option<&Vec<String>>) -> anyhow::Result<ListenOn> {
    let addresses = match addresses {
        Some(addrs) if !addrs.is_empty() => parse_acl_list(addrs)
            .with_context(|| format!("invalid entry in {directive} address list"))?,
        _ => vec![AddressMatchElement::Any],
    };
    Ok(ListenOn {
        port: Some(DNS_CONTAINER_PORT),
        addresses,
    })
}

/// `dnssec-validation auto` or `no`.
///
/// Enabled is `auto`, which validates with BIND's built-in root trust anchor.
/// `yes` needs `trust-anchors` configured, which bindy does not do: BIND 9.18
/// then validates nothing, and BIND 9.20 refuses to load the configuration.
fn dnssec_validation(enabled: bool) -> DnssecValidation {
    if enabled {
        return DnssecValidation::Auto;
    }
    DnssecValidation::No
}

/// Resolve `dnssec-validation` for the instance-level options builder: the
/// instance config overrides the cluster global config.
///
/// Whichever level configures `dnssec` first (instance, then global) renders
/// an explicit directive, regardless of whether the instance has a `config`
/// block at all (ADR-0007: an absent directive means `auto` to `named`,
/// which would silently re-enable validation a user explicitly disabled).
/// When neither level configures `dnssec`, no directive is emitted and
/// `named`'s own default applies.
///
/// # Arguments
///
/// * `instance_config` - The instance's `spec.config`, if any
/// * `global_config` - The cluster's `spec.common.global`, if any
fn resolve_dnssec_validation(
    instance_config: Option<&Bind9Config>,
    global_config: Option<&Bind9Config>,
) -> Option<DnssecValidation> {
    instance_config
        .and_then(|c| c.dnssec.as_ref())
        .or_else(|| global_config.and_then(|g| g.dnssec.as_ref()))
        .map(|dnssec| dnssec_validation(dnssec.validation.unwrap_or(false)))
}

/// An ACL directive's address match list (`allow-query`), or none.
///
/// `None` when `acls` is `None` or empty, so no directive is emitted.
/// `source` names the originating CRD field for error context.
///
/// # Errors
///
/// Returns an error if any entry fails address-match-list validation; see
/// [`crate::bind9_acl`] for the accepted syntax.
fn acl_list(acls: Option<&Vec<String>>, source: &str) -> anyhow::Result<Option<AddressMatchList>> {
    let Some(acls) = acls else {
        return Ok(None);
    };
    if acls.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        parse_acl_list(acls).with_context(|| format!("invalid entry in {source}"))?,
    ))
}

/// The `allow-transfer` list, where an explicitly **empty** list means `none`
/// (deny) rather than "not configured".
///
/// # Errors
///
/// Returns an error if any entry fails address-match-list validation; see
/// [`crate::bind9_acl`] for the accepted syntax.
fn transfer_acl(acls: &[String], source: &str) -> anyhow::Result<AddressMatchList> {
    if acls.is_empty() {
        return Ok(deny_all_transfers());
    }
    parse_acl_list(acls).with_context(|| format!("invalid entry in {source}"))
}
