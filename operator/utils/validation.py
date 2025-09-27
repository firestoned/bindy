"""
DNS record validation
"""

import re
from typing import Dict, Any


def validate_dns_record(record_type: str, spec: Dict[str, Any]) -> None:
    """Validate DNS record data"""

    validators = {
        "A": validate_a_record,
        "AAAA": validate_aaaa_record,
        "CNAME": validate_cname_record,
        "MX": validate_mx_record,
    }

    validator = validators.get(record_type)
    if validator:
        validator(spec)


def validate_a_record(spec: Dict[str, Any]) -> None:
    """Validate A record"""
    ipv4 = spec.get("ipv4Address")
    if not ipv4 or not re.match(r"^(\d{1,3}\.){3}\d{1,3}$", ipv4):
        raise ValueError(f"Invalid IPv4 address: {ipv4}")


def validate_aaaa_record(spec: Dict[str, Any]) -> None:
    """Validate AAAA record"""
    ipv6 = spec.get("ipv6Address")
    if not ipv6:
        raise ValueError("IPv6 address required")


def validate_cname_record(spec: Dict[str, Any]) -> None:
    """Validate CNAME record"""
    target = spec.get("target")
    if not target:
        raise ValueError("Target required for CNAME")


def validate_mx_record(spec: Dict[str, Any]) -> None:
    """Validate MX record"""
    if "priority" not in spec or "mailServer" not in spec:
        raise ValueError("Priority and mailServer required for MX record")
