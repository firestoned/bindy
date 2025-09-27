"""Tests for DNS record validation"""

import pytest
from operator.utils.validation import validate_a_record, validate_mx_record


def test_valid_a_record():
    spec = {"ipv4Address": "192.0.2.1"}
    validate_a_record(spec)  # Should not raise


def test_invalid_a_record():
    spec = {"ipv4Address": "999.999.999.999"}
    with pytest.raises(ValueError):
        validate_a_record(spec)


def test_valid_mx_record():
    spec = {"priority": 10, "mailServer": "mail.example.com"}
    validate_mx_record(spec)  # Should not raise


def test_invalid_mx_record():
    spec = {"priority": 10}  # Missing mailServer
    with pytest.raises(ValueError):
        validate_mx_record(spec)
