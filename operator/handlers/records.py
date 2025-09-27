"""
Handlers for DNS Record resources
"""

import kopf
import logging
from operator.kubernetes.resources import update_zone_file
from operator.utils.validation import validate_dns_record

logger = logging.getLogger(__name__)


def create_record_handler(record_type: str):
    """Factory function to create record handlers"""

    @kopf.on.create("dns.example.com", "v1alpha1", f"{record_type.lower()}records")
    @kopf.on.update("dns.example.com", "v1alpha1", f"{record_type.lower()}records")
    def handle_record(spec, name, namespace, logger, **kwargs):
        logger.info(f"Managing {record_type} record {name}")

        try:
            validate_dns_record(record_type, spec)

            zone_ref = spec.get("zone")
            update_zone_file(zone_ref, namespace, name, record_type, spec, logger)

            return {"status": "active", "recordType": record_type}

        except ValueError as e:
            raise kopf.PermanentError(f"Invalid record: {e}")
        except Exception as e:
            raise kopf.TemporaryError(f"Record management failed: {e}", delay=30)

    return handle_record


# Create handlers for all record types
for rtype in ["a", "aaaa", "cname", "mx", "txt", "ns", "srv", "ptr", "caa", "naptr"]:
    create_record_handler(rtype.upper())
