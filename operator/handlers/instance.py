"""
Handlers for BIND9 Instance resources
"""

import kopf
import logging
from operator.kubernetes.resources import (
    create_bind9_statefulset,
    create_bind9_services,
    create_configmap,
)
from operator.config.bind9 import generate_bind9_config
from operator.utils.dns import test_dns_functionality

logger = logging.getLogger(__name__)


@kopf.on.create("dns.example.com", "v1alpha1", "bind9instances")
@kopf.on.resume("dns.example.com", "v1alpha1", "bind9instances")
def create_bind9_instance(spec, name, namespace, uid, logger, **kwargs):
    """Create or ensure BIND9 instance exists"""
    logger.info(f"Creating BIND9 instance {name}")

    try:
        # Generate BIND9 configuration
        config_data = generate_bind9_config(spec, name, namespace)

        # Create ConfigMap for BIND9 configuration
        create_configmap(name, namespace, config_data, uid, "config")

        # Create StatefulSet for BIND9
        create_bind9_statefulset(spec, name, namespace, uid, logger)

        # Create Services
        create_bind9_services(name, namespace, uid)

        return {"phase": "Ready", "message": "BIND9 instance created successfully"}

    except Exception as e:
        logger.error(f"Failed to create BIND9 instance: {e}")
        raise kopf.TemporaryError(f"BIND9 creation failed: {e}", delay=30)


@kopf.on.update("dns.example.com", "v1alpha1", "bind9instances")
def update_bind9_instance(spec, name, namespace, uid, old, new, diff, logger, **kwargs):
    """Handle updates to BIND9 instance"""
    logger.info(f"Updating BIND9 instance {name}")

    try:
        # Regenerate configuration
        config_data = generate_bind9_config(spec, name, namespace)

        # Update ConfigMap
        create_configmap(name, namespace, config_data, uid, "config")

        # Update StatefulSet if needed
        if any(field in ["replicas", "image", "resources"] for field, _, _ in diff):
            create_bind9_statefulset(spec, name, namespace, uid, logger)

        return {"phase": "Ready", "message": "Updated successfully"}

    except Exception as e:
        raise kopf.TemporaryError(f"Update failed: {e}", delay=30)


@kopf.on.delete("dns.example.com", "v1alpha1", "bind9instances")
def delete_bind9_instance(spec, name, namespace, logger, **kwargs):
    """Handle deletion of BIND9 instance"""
    logger.info(f"Deleting BIND9 instance {name}")
    # Kubernetes will handle cleanup via owner references
    return {"message": "BIND9 instance deleted"}


@kopf.daemon("dns.example.com", "v1alpha1", "bind9instances")
def monitor_bind9_health(spec, name, namespace, stopped, logger, **kwargs):
    """Monitor BIND9 health"""
    logger.info(f"Starting health monitoring for {name}")

    while not stopped:
        try:
            if not test_dns_functionality(name, namespace, logger):
                logger.error(f"DNS functionality check failed for {name}")

            stopped.wait(60)

        except Exception as e:
            logger.exception(f"Error in monitoring: {e}")
            stopped.wait(120)
