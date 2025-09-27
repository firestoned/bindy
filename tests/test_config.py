"""Tests for BIND9 configuration generation"""

from operator.config.bind9 import generate_bind9_config, generate_zone_file


def test_generate_bind9_config():
    spec = {"config": {"recursion": False, "allowQuery": ["any"], "allowTransfer": ["none"]}}
    config = generate_bind9_config(spec, "test-instance", "default")

    assert "test-instance" in config
    assert "recursion no" in config
    assert "allow-query" in config


def test_generate_zone_file():
    spec = {
        "zoneName": "example.com",
        "ttl": 3600,
        "soaRecord": {
            "primaryNS": "ns1.example.com.",
            "adminEmail": "admin@example.com",
            "serial": 2024010101,
        },
    }

    zone = generate_zone_file(spec, "example.com", "default", None)

    assert "example.com" in zone
    assert "SOA" in zone
    assert "ns1.example.com." in zone
