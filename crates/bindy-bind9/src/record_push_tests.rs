// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `record_push.rs`

#[cfg(test)]
mod tests {
    // ========================================================================
    // Record replay after a BIND9 pod or Deployment is wiped (issue #486)
    // ========================================================================

    mod record_replay {
        use crate::record_push::RecordReplayOutcome;

        #[test]
        fn test_replay_outcome_default_is_complete() {
            // A zone with no records has nothing to replay and must not be
            // held back from Ready.
            let outcome = RecordReplayOutcome::default();

            assert!(outcome.is_complete());
            assert_eq!(outcome.attempted, 0);
            assert_eq!(outcome.succeeded, 0);
        }

        #[test]
        fn test_replay_outcome_with_failures_is_not_complete() {
            // Any record that did not reach BIND9 must keep the zone Degraded:
            // the server is authoritative and would answer NXDOMAIN for it.
            let outcome = RecordReplayOutcome {
                attempted: 3,
                succeeded: 2,
                skipped: 0,
                failures: vec!["ARecord default/www: connection refused".to_string()],
            };

            assert!(!outcome.is_complete());
        }

        #[test]
        fn test_replay_outcome_summary_reports_progress_on_success() {
            let outcome = RecordReplayOutcome {
                attempted: 4,
                succeeded: 4,
                skipped: 0,
                failures: vec![],
            };

            let summary = outcome.summary("example.com");

            assert!(summary.contains("4/4"), "summary was: {summary}");
            assert!(summary.contains("example.com"), "summary was: {summary}");
        }

        #[test]
        fn test_replay_outcome_summary_names_the_failures() {
            // The message lands in the Degraded condition, so it has to say
            // which records are missing from the zone.
            let outcome = RecordReplayOutcome {
                attempted: 2,
                succeeded: 1,
                skipped: 0,
                failures: vec!["ARecord default/www: connection refused".to_string()],
            };

            let summary = outcome.summary("example.com");

            assert!(summary.contains("1/2"), "summary was: {summary}");
            assert!(
                summary.contains("ARecord default/www"),
                "summary was: {summary}"
            );
            assert!(
                summary.contains("connection refused"),
                "summary was: {summary}"
            );
        }

        #[test]
        fn test_replay_outcome_counts_skipped_records_as_done() {
            // A record deleted while the zone was being recreated is not
            // missing data: it must not keep the zone Degraded.
            let outcome = RecordReplayOutcome {
                attempted: 3,
                succeeded: 2,
                skipped: 1,
                failures: vec![],
            };

            assert!(outcome.is_complete());
            assert!(outcome.summary("example.com").contains("3/3"));
        }

        #[test]
        fn test_should_replay_a_live_record() {
            let meta = k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta::default();

            assert!(crate::record_push::should_replay(&meta));
        }

        #[test]
        fn test_should_not_replay_a_terminating_record() {
            // Replaying a record whose finalizer is running would re-publish
            // the RRset after the finalizer deleted it (load test rc.2:
            // records still served after deletion).
            let meta = k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                deletion_timestamp: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                    k8s_openapi::jiff::Timestamp::now(),
                )),
                ..Default::default()
            };

            assert!(!crate::record_push::should_replay(&meta));
        }
    }

    // ========================================================================
    // Per-kind pins (ADR-0020): what each record kind is, and what its record
    // operation sends to BIND9 for a sample spec. Written against the
    // hand-written per-kind impls; the record-kind table must keep them green.
    // ========================================================================

    mod record_kind_pins {
        use crate::bind9::{Bind9Manager, RndcKeyData};
        use crate::crd::{
            AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, PTRRecord,
            RndcAlgorithm, SRVRecord, TXTRecord,
        };
        use crate::record_push::ReconcilableRecord;
        use hickory_proto::op::{Message, OpCode};
        use hickory_proto::rr::{rdata, Name, RData, Record, RecordType};
        use serde_json::json;
        use std::str::FromStr;
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        /// Zone every sample record is published into.
        const ZONE: &str = "example.com";
        /// TTL every sample spec sets.
        const TTL: u32 = 300;
        /// Upper bound for one write against the local fake server.
        const WRITE_DEADLINE: Duration = Duration::from_secs(5);
        /// Largest DNS message the fake server reads.
        const MAX_DNS_MESSAGE: usize = 4096;

        // --- adapter: the only part that names the trait's methods --------

        fn short_name<T: ReconcilableRecord>() -> String {
            crate::record_push::record_type::<T>().to_string()
        }

        fn hickory<T: ReconcilableRecord>() -> RecordType {
            T::DNS_KIND.to_hickory_record_type()
        }

        fn record_name<T: ReconcilableRecord>(record: &T) -> String {
            record.spec_name().to_string()
        }

        fn ttl<T: ReconcilableRecord>(record: &T) -> Option<i32> {
            record.ttl()
        }

        #[allow(deprecated)] // `status.zone` is a plain field to read back
        fn status_zone<T: ReconcilableRecord>(record: &T) -> Option<String> {
            record.record_status().and_then(|s| s.zone.clone())
        }

        fn display<T: ReconcilableRecord>(record: &T) -> Option<String> {
            record.display_addresses()
        }

        async fn push<T: ReconcilableRecord>(record: &T, server: &str) -> anyhow::Result<()> {
            // `Bind9Manager::new()` builds a reqwest client, which needs a
            // rustls provider; `install_default` errs once one is set.
            let _ = rustls::crypto::ring::default_provider().install_default();
            record
                .add_to_bind9(&Bind9Manager::new(), ZONE, server, &key())
                .await
        }

        // --- fake BIND9 ------------------------------------------------------

        fn key() -> RndcKeyData {
            RndcKeyData {
                name: "bindy-test".to_string(),
                algorithm: RndcAlgorithm::HmacSha256,
                secret: "c2VjcmV0LXNlY3JldC1zZWNyZXQtc2VjcmV0LXNlY3JldA==".to_string(),
            }
        }

        /// What the fake server saw.
        #[derive(Default)]
        struct Seen {
            queries: Vec<(Name, RecordType)>,
            updates: usize,
        }

        /// A UDP DNS server on localhost that answers every query with the
        /// `answers` of the queried type and counts UPDATE messages without
        /// replying to them. A write whose desired `RRset` equals `answers`
        /// queries once and sends no UPDATE: that is how these tests see the
        /// exact data an operation would publish.
        async fn fake_bind9(answers: Vec<Record>) -> (String, Arc<Mutex<Seen>>) {
            let socket = tokio::net::UdpSocket::bind("127.0.0.1:0")
                .await
                .expect("bind fake DNS server");
            let addr = socket.local_addr().expect("local addr").to_string();
            let seen = Arc::new(Mutex::new(Seen::default()));
            let seen_by_server = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut buf = vec![0u8; MAX_DNS_MESSAGE];
                loop {
                    let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                        return;
                    };
                    let Ok(request) = Message::from_vec(&buf[..len]) else {
                        continue;
                    };
                    if request.metadata.op_code != OpCode::Query {
                        seen_by_server.lock().expect("lock").updates += 1;
                        continue;
                    }
                    let mut response = Message::response(request.metadata.id, OpCode::Query);
                    response.metadata.recursion_desired = request.metadata.recursion_desired;
                    for query in &request.queries {
                        seen_by_server
                            .lock()
                            .expect("lock")
                            .queries
                            .push((query.name().clone(), query.query_type()));
                        response.answers.extend(
                            answers
                                .iter()
                                .filter(|r| r.record_type() == query.query_type())
                                .cloned(),
                        );
                    }
                    response.queries.clone_from(&request.queries);
                    let Ok(bytes) = response.to_vec() else {
                        continue;
                    };
                    let _ = socket.send_to(&bytes, peer).await;
                }
            });
            (addr, seen)
        }

        fn fixture<T: serde::de::DeserializeOwned>(kind: &str, spec: serde_json::Value) -> T {
            serde_json::from_value(json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": kind,
                "metadata": {"name": "rec", "namespace": "team"},
                "spec": spec,
                "status": {"zone": ZONE},
            }))
            .expect("valid fixture")
        }

        fn name(s: &str) -> Name {
            Name::from_str(s).expect("valid name")
        }

        fn rr(owner: &str, data: RData) -> Record {
            Record::from_rdata(name(owner), TTL, data)
        }

        /// One kind's expected facts.
        struct Expected {
            short: &'static str,
            record_type: RecordType,
            record_name: &'static str,
            fqdn: &'static str,
            display: Option<&'static str>,
            published: Vec<Record>,
        }

        async fn assert_kind<T: ReconcilableRecord>(
            kind: &str,
            spec: serde_json::Value,
            expected: Expected,
        ) {
            let record: T = fixture(kind, spec);

            assert_eq!(short_name::<T>(), expected.short, "{kind} type name");
            assert_eq!(hickory::<T>(), expected.record_type, "{kind} hickory type");
            assert_eq!(record_name(&record), expected.record_name, "{kind} name");
            assert_eq!(ttl(&record), Some(300), "{kind} ttl");
            assert_eq!(status_zone(&record).as_deref(), Some(ZONE), "{kind} status");
            assert_eq!(
                display(&record).as_deref(),
                expected.display,
                "{kind} display"
            );

            let (server, seen) = fake_bind9(expected.published).await;
            let result = tokio::time::timeout(WRITE_DEADLINE, push(&record, &server)).await;
            assert!(
                matches!(result, Ok(Ok(()))),
                "{kind}: the write must find its exact RRset already published"
            );
            let seen = seen.lock().expect("lock");
            assert_eq!(seen.updates, 0, "{kind}: no UPDATE for an unchanged RRset");
            assert_eq!(
                seen.queries,
                vec![(name(expected.fqdn), expected.record_type)],
                "{kind}: queried name and type"
            );
        }

        #[tokio::test]
        async fn a_record() {
            assert_kind::<ARecord>(
                "ARecord",
                json!({"name": "www", "ipv4Addresses": ["192.0.2.10", "192.0.2.11"], "ttl": 300}),
                Expected {
                    short: "A",
                    record_type: RecordType::A,
                    record_name: "www",
                    fqdn: "www.example.com.",
                    display: Some("192.0.2.10,192.0.2.11"),
                    published: vec![
                        rr(
                            "www.example.com.",
                            RData::A("192.0.2.10".parse().expect("ip")),
                        ),
                        rr(
                            "www.example.com.",
                            RData::A("192.0.2.11".parse().expect("ip")),
                        ),
                    ],
                },
            )
            .await;
        }

        #[tokio::test]
        async fn aaaa_record() {
            assert_kind::<AAAARecord>(
                "AAAARecord",
                json!({"name": "v6", "ipv6Addresses": ["2001:db8::1"], "ttl": 300}),
                Expected {
                    short: "AAAA",
                    record_type: RecordType::AAAA,
                    record_name: "v6",
                    fqdn: "v6.example.com.",
                    display: Some("2001:db8::1"),
                    published: vec![rr(
                        "v6.example.com.",
                        RData::AAAA("2001:db8::1".parse().expect("ip")),
                    )],
                },
            )
            .await;
        }

        #[tokio::test]
        async fn txt_record() {
            assert_kind::<TXTRecord>(
                "TXTRecord",
                json!({"name": "@", "text": ["v=spf1 -all"], "ttl": 300}),
                Expected {
                    short: "TXT",
                    record_type: RecordType::TXT,
                    record_name: "@",
                    fqdn: "example.com.",
                    display: None,
                    published: vec![rr(
                        "example.com.",
                        RData::TXT(rdata::TXT::new(vec!["v=spf1 -all".to_string()])),
                    )],
                },
            )
            .await;
        }

        #[tokio::test]
        async fn cname_record() {
            assert_kind::<CNAMERecord>(
                "CNAMERecord",
                json!({"name": "alias", "target": "www.example.com.", "ttl": 300}),
                Expected {
                    short: "CNAME",
                    record_type: RecordType::CNAME,
                    record_name: "alias",
                    fqdn: "alias.example.com.",
                    display: None,
                    published: vec![rr(
                        "alias.example.com.",
                        RData::CNAME(rdata::CNAME(name("www.example.com."))),
                    )],
                },
            )
            .await;
        }

        #[tokio::test]
        async fn mx_record() {
            assert_kind::<MXRecord>(
                "MXRecord",
                json!({"name": "@", "priority": 10, "mailServer": "mail.example.com.", "ttl": 300}),
                Expected {
                    short: "MX",
                    record_type: RecordType::MX,
                    record_name: "@",
                    fqdn: "example.com.",
                    display: None,
                    published: vec![rr(
                        "example.com.",
                        RData::MX(rdata::MX::new(10, name("mail.example.com."))),
                    )],
                },
            )
            .await;
        }

        #[tokio::test]
        async fn ns_record() {
            assert_kind::<NSRecord>(
                "NSRecord",
                json!({"name": "sub", "nameserver": "ns1.sub.example.com.", "ttl": 300}),
                Expected {
                    short: "NS",
                    record_type: RecordType::NS,
                    record_name: "sub",
                    fqdn: "sub.example.com.",
                    display: None,
                    published: vec![rr(
                        "sub.example.com.",
                        RData::NS(rdata::NS(name("ns1.sub.example.com."))),
                    )],
                },
            )
            .await;
        }

        #[tokio::test]
        async fn srv_record() {
            assert_kind::<SRVRecord>(
                "SRVRecord",
                json!({"name": "_sip._tcp", "priority": 10, "weight": 20, "port": 5060,
                       "target": "sip.example.com.", "ttl": 300}),
                Expected {
                    short: "SRV",
                    record_type: RecordType::SRV,
                    record_name: "_sip._tcp",
                    fqdn: "_sip._tcp.example.com.",
                    display: None,
                    published: vec![rr(
                        "_sip._tcp.example.com.",
                        RData::SRV(rdata::SRV::new(10, 20, 5060, name("sip.example.com."))),
                    )],
                },
            )
            .await;
        }

        #[tokio::test]
        async fn caa_record() {
            assert_kind::<CAARecord>(
                "CAARecord",
                json!({"name": "@", "flags": 0, "tag": "issue", "value": "letsencrypt.org", "ttl": 300}),
                Expected {
                    short: "CAA",
                    record_type: RecordType::CAA,
                    record_name: "@",
                    fqdn: "example.com.",
                    display: None,
                    published: vec![rr(
                        "example.com.",
                        RData::CAA(rdata::CAA::new_issue(
                            false,
                            Some(name("letsencrypt.org")),
                            Vec::new(),
                        )),
                    )],
                },
            )
            .await;
        }

        #[tokio::test]
        async fn ptr_record() {
            assert_kind::<PTRRecord>(
                "PTRRecord",
                json!({"name": "10", "target": "www.example.com.", "ttl": 300}),
                Expected {
                    short: "PTR",
                    record_type: RecordType::PTR,
                    record_name: "10",
                    fqdn: "10.example.com.",
                    display: None,
                    published: vec![rr(
                        "10.example.com.",
                        RData::PTR(rdata::PTR(name("www.example.com."))),
                    )],
                },
            )
            .await;
        }
    }
}
