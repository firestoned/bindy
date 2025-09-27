"""
Configuration templates for BIND9
"""

BIND9_CONFIG_TEMPLATE = """
// Generated BIND9 configuration for {instance_name}
options {{
    directory "/var/lib/bind";
    listen-on {{ any; }};
    listen-on-v6 {{ any; }};

    allow-query {{ {allow_query}; }};
    allow-transfer {{ {allow_transfer}; }};
    recursion {recursion};

    dnssec-validation {dnssec_validation};

    version none;
    hostname none;
    server-id none;
}};

logging {{
    channel default_log {{
        file "/var/log/bind/named.log" versions 3 size 5m;
        severity info;
        print-time yes;
        print-severity yes;
        print-category yes;
    }};

    category default {{ default_log; }};
    category queries {{ default_log; }};
}};

include "/etc/bind/zones/*.conf";
"""

ZONE_FILE_TEMPLATE = """
$ORIGIN {zone_name}.
$TTL {ttl}

@ IN SOA {primary_ns} {admin_email} (
    {serial}    ; Serial
    {refresh}   ; Refresh
    {retry}     ; Retry
    {expire}    ; Expire
    {minimum}   ; Negative TTL
)

; Records will be added here
"""
