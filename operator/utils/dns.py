"""
DNS utility functions
"""

import dns.resolver
import logging

logger = logging.getLogger(__name__)


def test_dns_functionality(instance_name: str, namespace: str, logger) -> bool:
    """Test if DNS is responding"""
    try:
        resolver = dns.resolver.Resolver()
        resolver.nameservers = [f"{instance_name}-dns.{namespace}.svc.cluster.local"]
        resolver.query(".", "NS")
        return True
    except Exception as e:
        logger.error(f"DNS test failed: {e}")
        return False
