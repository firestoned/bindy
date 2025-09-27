"""
Main entry point for the BIND9 DNS Operator
"""

import logging
import sys
from pathlib import Path
import kopf
import os

# Import handlers
from operator.handlers import instance, zone, records

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    handlers=[
        logging.StreamHandler(sys.stdout),
    ],
)

logger = logging.getLogger(__name__)


@kopf.on.startup()
def configure_operator(settings: kopf.OperatorSettings, **_):
    """Configure operator for production use"""

    # Watching configuration
    settings.watching.server_timeout = 60
    settings.watching.client_timeout = 120

    # Posting configuration
    settings.posting.enabled = True
    settings.posting.level = logging.INFO

    # Peering configuration
    settings.peering.name = "bind9-dns-operator"
    settings.peering.mandatory = True

    # Execution configuration
    settings.execution.max_workers = 10

    logger.info("BIND9 DNS Operator configured successfully")


@kopf.on.login()
def login(**kwargs):
    """Handle authentication"""
    if os.getenv("KOPF_ENV") == "development":
        return kopf.login_via_client(**kwargs)
    else:
        return kopf.login_via_client(**kwargs)


def main():
    """Main entry point"""
    kopf.run(
        standalone=True,
        namespace=os.getenv("OPERATOR_NAMESPACE"),
    )


if __name__ == "__main__":
    main()
