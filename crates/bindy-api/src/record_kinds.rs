// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The record-kind table: every DNS record kind, declared once (ADR-0020).
//!
//! [`for_each_record_kind!`](crate::for_each_record_kind) holds the one list
//! of the nine record kinds and the facts that differ between them: the
//! [`DNSRecordKind`] variant (which is also the hickory `RecordType` name),
//! the CRD type, the kind string and the finalizer. Everything else about a
//! kind is generated from it:
//!
//! - here, [`DNSRecordKind`] and its conversions;
//! - in `bindy-controller-sdk`, the `RecordKind` impls and the
//!   `RECORD_KINDS` registry;
//! - in every crate that iterates or dispatches over the kinds (the binaries,
//!   bootstrap, the record and zone controllers, the BIND9 replay), through
//!   [`each_record_kind!`](crate::each_record_kind),
//!   [`match_record_kind!`](crate::match_record_kind) and
//!   [`record_kind_array!`](crate::record_kind_array).
//!
//! Adding a record kind is its CRD struct in [`crate::crd`], one row below,
//! and its hand-written DNS data conversion in `bindy-bind9`. A kind missing
//! that conversion fails to compile wherever the table is iterated.

/// The table of DNS record kinds, passed to a callback macro.
///
/// Invokes `callback! { args @rows (Variant, Type, "Kind", "finalizer"), ... }`
/// with one row per record kind, in the order record discovery reports them
/// (the order of [`DNSRecordKind::all`]). The callback is given in brackets so
/// it may be a path, e.g. `$crate::each_record_kind`; `args` are passed
/// through unchanged before the `@rows` marker.
///
/// A callback should match a row as `($variant:ident, $record:ty,
/// $($rest:tt)*)` unless it needs the later columns, so a new column does not
/// break it.
///
/// Most crates use the helpers built on it instead:
/// [`each_record_kind!`](crate::each_record_kind),
/// [`match_record_kind!`](crate::match_record_kind) and
/// [`record_kind_array!`](crate::record_kind_array).
#[macro_export]
macro_rules! for_each_record_kind {
    ([$($callback:tt)+] $($args:tt)*) => {
        $($callback)+! {
            $($args)*
            @rows
            (A, $crate::crd::ARecord, "ARecord", "bindy.firestoned.io/arecord-finalizer"),
            (AAAA, $crate::crd::AAAARecord, "AAAARecord", "bindy.firestoned.io/aaaarecord-finalizer"),
            (TXT, $crate::crd::TXTRecord, "TXTRecord", "bindy.firestoned.io/txtrecord-finalizer"),
            (CNAME, $crate::crd::CNAMERecord, "CNAMERecord", "bindy.firestoned.io/cnamerecord-finalizer"),
            (MX, $crate::crd::MXRecord, "MXRecord", "bindy.firestoned.io/mxrecord-finalizer"),
            (NS, $crate::crd::NSRecord, "NSRecord", "bindy.firestoned.io/nsrecord-finalizer"),
            (SRV, $crate::crd::SRVRecord, "SRVRecord", "bindy.firestoned.io/srvrecord-finalizer"),
            (CAA, $crate::crd::CAARecord, "CAARecord", "bindy.firestoned.io/caarecord-finalizer"),
            (PTR, $crate::crd::PTRRecord, "PTRRecord", "bindy.firestoned.io/ptrrecord-finalizer"),
        }
    };
}

/// Run `body` once per record kind, in table order, with `K` naming the
/// kind's CRD type.
///
/// Expands to one block per kind, `{ type K = ARecord; body; }`, so `body`
/// may use `?` and `.await` of the enclosing function.
///
/// ```rust,ignore
/// bindy_api::each_record_kind!(K => crds.push(build_crd::<K>()?));
/// ```
#[macro_export]
macro_rules! each_record_kind {
    (@each $k:ident ($body:expr) @rows $( ($variant:ident, $record:ty, $($rest:tt)*) ),* $(,)?) => {
        $( { type $k = $record; $body; } )*
    };
    ($k:ident => $body:expr) => {
        $crate::for_each_record_kind!([$crate::each_record_kind] @each $k ($body))
    };
}

/// Match a [`DNSRecordKind`] value and evaluate `body` with `K` naming that
/// kind's CRD type.
///
/// ```rust,ignore
/// bindy_api::match_record_kind!(kind, K => list_names::<K>(client, ns).await)
/// ```
#[macro_export]
macro_rules! match_record_kind {
    (@match ($kind:expr) $k:ident ($body:expr) @rows $( ($variant:ident, $record:ty, $($rest:tt)*) ),* $(,)?) => {
        match $kind {
            $( $crate::record_kinds::DNSRecordKind::$variant => { type $k = $record; $body } )*
        }
    };
    ($kind:expr, $k:ident => $body:expr) => {
        $crate::for_each_record_kind!([$crate::match_record_kind] @match ($kind) $k ($body))
    };
}

/// An array with one `body` per record kind, in table order, with `K` naming
/// the kind's CRD type. Usable in a `const` initializer.
///
/// ```rust,ignore
/// const OPS: [RecordKindOps; DNSRecordKind::COUNT] = bindy_api::record_kind_array!(K => ops::<K>());
/// ```
#[macro_export]
macro_rules! record_kind_array {
    (@array $k:ident ($body:expr) @rows $( ($variant:ident, $record:ty, $($rest:tt)*) ),* $(,)?) => {
        [ $( { type $k = $record; $body } ),* ]
    };
    ($k:ident => $body:expr) => {
        $crate::for_each_record_kind!([$crate::record_kind_array] @array $k ($body))
    };
}

/// Generates [`DNSRecordKind`] and its conversions from the table.
macro_rules! dns_record_kind {
    (@rows $( ($variant:ident, $record:ty, $kind:literal, $finalizer:literal) ),* $(,)?) => {
        /// DNS record kind (type) enumeration.
        ///
        /// Represents the Kubernetes `kind` field for all DNS record custom
        /// resources, with type-safe conversions between the kind string,
        /// the enum value and the hickory `RecordType`. Generated from the
        /// record-kind table ([`crate::for_each_record_kind`]).
        ///
        /// # Example
        ///
        /// ```rust
        /// use bindy_api::crd::DNSRecordKind;
        ///
        /// // Parse from string (fallible: unknown kinds return Err instead of panicking)
        /// let kind = DNSRecordKind::try_from("ARecord").unwrap();
        /// assert_eq!(kind, DNSRecordKind::A);
        ///
        /// // Convert to string
        /// assert_eq!(kind.as_str(), "ARecord");
        /// ```
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum DNSRecordKind {
            $(
                #[doc = concat!("The `", $kind, "` kind (DNS type ", stringify!($variant), ")")]
                $variant,
            )*
        }

        impl DNSRecordKind {
            /// Number of record kinds.
            pub const COUNT: usize = Self::all().len();

            /// Returns the Kubernetes `kind` string for this record type.
            ///
            /// ```rust
            /// use bindy_api::crd::DNSRecordKind;
            ///
            /// assert_eq!(DNSRecordKind::A.as_str(), "ARecord");
            /// assert_eq!(DNSRecordKind::MX.as_str(), "MXRecord");
            /// ```
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $kind, )*
                }
            }

            /// Returns all DNS record kinds, in the order record discovery
            /// reports them.
            #[must_use]
            pub const fn all() -> &'static [Self] {
                &[ $( Self::$variant, )* ]
            }

            /// Converts this DNS record kind to a Hickory DNS `RecordType`.
            ///
            /// ```rust
            /// use bindy_api::crd::DNSRecordKind;
            /// use hickory_proto::rr::RecordType;
            ///
            /// assert_eq!(DNSRecordKind::A.to_hickory_record_type(), RecordType::A);
            /// ```
            #[must_use]
            pub const fn to_hickory_record_type(self) -> hickory_proto::rr::RecordType {
                match self {
                    $( Self::$variant => hickory_proto::rr::RecordType::$variant, )*
                }
            }

            /// The finalizer the record controller keeps on resources of this
            /// kind.
            ///
            /// ```rust
            /// use bindy_api::crd::DNSRecordKind;
            ///
            /// assert_eq!(DNSRecordKind::A.finalizer(), "bindy.firestoned.io/arecord-finalizer");
            /// ```
            #[must_use]
            pub const fn finalizer(self) -> &'static str {
                match self {
                    $( Self::$variant => $finalizer, )*
                }
            }
        }

        impl TryFrom<&str> for DNSRecordKind {
            type Error = UnknownDNSRecordKind;

            fn try_from(s: &str) -> Result<Self, Self::Error> {
                match s {
                    $( $kind => Ok(Self::$variant), )*
                    _ => Err(UnknownDNSRecordKind(s.to_string())),
                }
            }
        }
    };
}

crate::for_each_record_kind!([dns_record_kind]);

/// Error returned when a string does not match a known [`DNSRecordKind`].
///
/// The fallible `TryFrom` impls surface this error so the caller can return
/// it through normal `Result` propagation rather than crashing the
/// reconciler.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownDNSRecordKind(pub String);

impl std::fmt::Display for UnknownDNSRecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let expected: Vec<&str> = DNSRecordKind::all()
            .iter()
            .map(|kind| kind.as_str())
            .collect();
        write!(
            f,
            "unknown DNS record kind {:?} (expected one of: {})",
            self.0,
            expected.join(", ")
        )
    }
}

impl std::error::Error for UnknownDNSRecordKind {}

impl TryFrom<String> for DNSRecordKind {
    type Error = UnknownDNSRecordKind;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(s.as_str())
    }
}

impl std::str::FromStr for DNSRecordKind {
    type Err = UnknownDNSRecordKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl std::fmt::Display for DNSRecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
#[path = "record_kinds_tests.rs"]
mod record_kinds_tests;
