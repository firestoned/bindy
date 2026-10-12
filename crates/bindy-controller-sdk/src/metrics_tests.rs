// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `metrics.rs`

#[cfg(test)]
mod tests {
    use super::super::{record_resource_deleted, RESOURCES_ACTIVE};

    /// Nothing increments `RESOURCES_ACTIVE`, so a deletion must not
    /// decrement it: the gauge would otherwise go negative.
    #[test]
    fn test_record_resource_deleted_does_not_drive_active_gauge_negative() {
        let resource_type = "DeletedOnlyResource";

        record_resource_deleted(resource_type);
        record_resource_deleted(resource_type);

        let active = RESOURCES_ACTIVE.with_label_values(&[resource_type]).get();
        assert!(
            active >= 0.0,
            "resources_active must never be negative, got {active}"
        );
    }
}
