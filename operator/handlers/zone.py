"""
Handlers for DNS Zone resources
"""

import kopf
import logging
from operator.config.bind9 import generate_zone_file
from operator.kubernetes.resources import create_configmap, reload_bind9_config
from operator.kubernetes.client import get_k8s_client
from datetime import datetime

logger = logging.getLogger(__name__)


@kopf.on.create("dns.example.com", "v1alpha1", "dnszones")
@kopf.on.update("dns.example.com", "v1alpha1", "dnszones")
@kopf.on.resume("dns.example.com", "v1alpha1", "dnszones")
def manage_dns_zone(spec, name, namespace, uid, logger, **kwargs):
    """Manage DNS zone configuration"""
    zone_name = spec.get("zoneName")
    logger.info(f"Managing DNS zone {zone_name}")

    try:
        # Generate zone file content
        zone_content = generate_zone_file(spec, zone_name, namespace, logger)

        # Create zone file ConfigMap
        create_configmap(name, namespace, zone_content, uid, "zone")

        # Find and reload BIND9 instance
        bind_instance = spec.get("bind9InstanceRef", "default")
        reload_bind9_config(bind_instance, namespace, logger)

        serial = int(datetime.now().strftime("%Y%m%d01"))

        return {"phase": "Active", "serial": serial, "zoneName": zone_name}

    except Exception as e:
        raise kopf.TemporaryError(f"Zone management failed: {e}", delay=30)


@kopf.on.delete("dns.example.com", "v1alpha1", "dnszones")
def delete_dns_zone(spec, name, namespace, logger, **kwargs):
    """Handle zone deletion"""
    logger.info(f"Deleting DNS zone {name}")

    bind_instance = spec.get("bind9InstanceRef", "default")
    reload_bind9_config(bind_instance, namespace, logger)

    return {"message": "Zone deleted"}
