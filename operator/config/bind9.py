"""
BIND9 configuration generation
"""

from typing import Dict, Any
from datetime import datetime
from operator.config.templates import BIND9_CONFIG_TEMPLATE, ZONE_FILE_TEMPLATE


def generate_bind9_config(spec: Dict[str, Any], instance_name: str, namespace: str) -> str:
    """Generate BIND9 named.conf configuration"""

    config = spec.get("config", {})

    return BIND9_CONFIG_TEMPLATE.format(
        instance_name=instance_name,
        allow_query=" ".join(config.get("allowQuery", ["any"])),
        allow_transfer=" ".join(config.get("allowTransfer", ["none"])),
        recursion="yes" if config.get("recursion", False) else "no",
        dnssec_validation="auto" if config.get("dnssec", {}).get("enabled", False) else "no",
    )


def generate_zone_file(spec: Dict[str, Any], zone_name: str, namespace: str, logger) -> str:
    """Generate BIND9 zone file"""

    soa = spec.get("soaRecord", {})
    serial = soa.get("serial", int(datetime.now().strftime("%Y%m%d01")))

    return ZONE_FILE_TEMPLATE.format(
        zone_name=zone_name,
        ttl=spec.get("ttl", 3600),
        primary_ns=soa.get("primaryNS", f"ns1.{zone_name}."),
        admin_email=soa.get("adminEmail", f"admin.{zone_name}.").replace("@", "."),
        serial=serial,
        refresh=soa.get("refresh", 3600),
        retry=soa.get("retry", 600),
        expire=soa.get("expire", 604800),
        minimum=soa.get("negativeTTL", 86400),
    )
