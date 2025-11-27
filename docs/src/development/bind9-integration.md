# BIND9 Integration

How Bindy integrates with BIND9 DNS server.

## Configuration Generation

Bindy generates BIND9 configuration from Bind9Instance specs:

### named.conf
```
options {
    directory "/var/lib/bind";
    recursion no;
    allow-query { 0.0.0.0/0; };
};

zone "example.com" {
    type master;
    file "/var/lib/bind/zones/example.com.zone";
};
```

### Zone Files
```
$TTL 3600
@   IN  SOA ns1.example.com. admin.example.com. (
        2024010101  ; serial
        3600        ; refresh
        600         ; retry
        604800      ; expire
        86400 )     ; negative TTL
    IN  NS  ns1.example.com.
www IN  A   192.0.2.1
```

## Zone File Management

Operations:
- Create new zones
- Add/update records
- Increment serial numbers
- Reload BIND9 configuration

## BIND9 Lifecycle

1. **ConfigMap** - Contains configuration files
2. **Volume Mount** - Mount ConfigMap to BIND9 pod
3. **Init** - BIND9 starts with configuration
4. **Reload** - `rndc reload` when configuration changes

## Future Enhancements

- Dynamic DNS updates (nsupdate)
- TSIG key management
- Zone transfer monitoring
- Query statistics collection
