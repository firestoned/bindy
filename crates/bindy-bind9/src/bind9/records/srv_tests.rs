// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for SRV record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::records::needs_update;
    use hickory_proto::rr::{rdata, Name, RData, Record};
    use std::str::FromStr;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;
    const TEST_PRIORITY: u16 = 10;
    const TEST_WEIGHT: u16 = 60;
    const TEST_PORT: u16 = 5060;
    const OTHER_PORT: u16 = 5061;

    fn make_srv_record(priority: u16, weight: u16, port: u16, target: &str, ttl: u32) -> Record {
        let name = Name::from_str("_sip._tcp.example.com.").expect("valid test name");
        let target_name = Name::from_str(target).expect("valid test target");
        Record::from_rdata(
            name,
            ttl,
            RData::SRV(rdata::SRV::new(priority, weight, port, target_name)),
        )
    }

    fn srv(priority: i32, weight: i32, port: i32, target: &str) -> SRVRecordData {
        SRVRecordData {
            priority,
            weight,
            port,
            target: target.to_string(),
            ttl: None,
        }
    }

    fn default_srv(target: &str) -> SRVRecordData {
        srv(
            i32::from(TEST_PRIORITY),
            i32::from(TEST_WEIGHT),
            i32::from(TEST_PORT),
            target,
        )
    }

    fn live(port: u16, target: &str) -> Vec<Record> {
        vec![make_srv_record(
            TEST_PRIORITY,
            TEST_WEIGHT,
            port,
            target,
            TEST_TTL,
        )]
    }

    // ========== srv_rdata ==========

    #[test]
    fn test_srv_rdata_is_one_value() {
        let rdata = srv_rdata(&default_srv("sip.example.com.")).expect("valid");
        assert_eq!(
            rdata,
            vec![RData::SRV(rdata::SRV::new(
                TEST_PRIORITY,
                TEST_WEIGHT,
                TEST_PORT,
                Name::from_str("sip.example.com.").expect("valid")
            ))]
        );
    }

    #[test]
    fn test_srv_rdata_rejects_out_of_range_fields() {
        let too_big = i32::from(u16::MAX) + 1;
        for (data, field) in [
            (srv(-1, 0, 0, "sip.example.com."), "priority"),
            (srv(0, too_big, 0, "sip.example.com."), "weight"),
            (srv(0, 0, -1, "sip.example.com."), "port"),
        ] {
            let err = srv_rdata(&data).expect_err("out of range");
            assert!(
                format!("{err:#}").contains(&format!("Invalid SRV {field}")),
                "{err:#}"
            );
        }
    }

    #[test]
    fn test_srv_rdata_rejects_a_bad_target() {
        let err = srv_rdata(&default_srv("sip..example.com")).expect_err("empty label");
        assert!(format!("{err:#}").contains("sip..example.com"), "{err:#}");
    }

    // ========== value comparison through needs_update ==========

    #[test]
    fn test_srv_same_values_and_ttl_need_no_update() {
        let desired = srv_rdata(&default_srv("SIP.example.com")).expect("valid");
        assert!(!needs_update(
            &live(TEST_PORT, "sip.example.com."),
            &desired,
            TEST_TTL
        ));
    }

    #[test]
    fn test_srv_changed_port_needs_update() {
        let desired = srv_rdata(&default_srv("sip.example.com.")).expect("valid");
        assert!(needs_update(
            &live(OTHER_PORT, "sip.example.com."),
            &desired,
            TEST_TTL
        ));
    }

    #[test]
    fn test_srv_changed_target_needs_update() {
        let desired = srv_rdata(&default_srv("sip2.example.com.")).expect("valid");
        assert!(needs_update(
            &live(TEST_PORT, "sip1.example.com."),
            &desired,
            TEST_TTL
        ));
    }

    #[test]
    fn test_srv_ttl_only_change_needs_update() {
        let desired = srv_rdata(&default_srv("sip.example.com.")).expect("valid");
        assert!(needs_update(
            &live(TEST_PORT, "sip.example.com."),
            &desired,
            OTHER_TTL
        ));
    }
}
