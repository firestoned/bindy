# Renaming DNSZones and Records

A resource's `metadata.name` is immutable in Kubernetes, so "renaming" a
`DNSZone` or a record means deleting it and creating it again under the new
name. Done naively, that takes DNS data offline. This page explains why, and
how to rename without an outage.

A common reason to rename is to make names say what they represent: a
`DNSZone` called `example.com` rather than `example-com`, an `ARecord` called
`www.example.com` rather than `example-com-www`. Dots are valid in these
names, and names are unique per kind, so an `ARecord`, an `MXRecord` and a
`DNSZone` can all be named `example.com`.

## Why a plain delete and re-create is not safe

Every `DNSZone` and record carries a bindy finalizer, and the finalizer does
real work when the resource is deleted:

- **Deleting a record** removes that name's whole RRset (name and type) from
  every primary.
- **Deleting a `DNSZone`** deletes the zone from every primary, and the
  secondaries lose it at their next transfer.

Creating the new resources *first* does not help either:

- **A second `DNSZone` for the same `zoneName` is refused.** The older
  resource keeps the zone and the newer one reports a `DuplicateZone`
  condition until the older one is gone.
- **Two resources for the same record do no harm while both exist**, but
  deleting either one normally runs its finalizer, which removes the RRset
  that the other one still declares.

## Renaming a zone and its records together (no outage)

Remove the old resources **without** running their finalizers, then apply the
renamed ones:

1. BIND keeps serving the zone and every record throughout, because no
   finalizer runs.
2. The old `DNSZone` takes its status with it, so the stale-record cleanup
   has nothing to act on.
3. The new `DNSZone` adopts the zone that is already loaded, and the new
   records rewrite RRsets identical to the ones being served.

```bash
NS=bindy-system
OLD="dnszone/example-com arecord/example-com-www arecord/example-com-apex mxrecord/example-com-mx"

# 1. Strip the finalizers, so deletion leaves BIND untouched
for r in $OLD; do
  kubectl -n "$NS" patch "$r" --type=merge -p '{"metadata":{"finalizers":null}}'
done

# 2. Delete the old resources
kubectl -n "$NS" delete $OLD

# 3. Apply the same objects under their new names
kubectl apply -f example.com.yaml
```

The new manifest keeps `spec` unchanged: the same `zoneName`, the same record
`spec.name` values, and the same labels the zone's `recordsFrom` selector
matches. Only `metadata.name` changes.

!!! warning "Strip finalizers only as part of a swap"
    A resource deleted without its finalizer leaves its DNS data in BIND, and
    nothing will remove it later. That is the point here, because the
    renamed resource takes ownership of the same data. Never strip a
    finalizer to delete something you actually want gone.

Then check that the zone and its records are back to `Ready` and that every
server still answers:

```bash
kubectl -n "$NS" get dnszones,arecords,mxrecords
dig +short @<primary-address> www.example.com A
dig +short @<primary-address> example.com MX
```

The zone's SOA serial increases as bindy rewrites the records, and with
DNSSEC signing the zone is re-signed. When the keys come from
`keysFrom.secretRef` they do not change, so `DNSZone.status.dnssec.dsRecords`
stays the same and no parent-zone change is needed.

## Renaming records only, under a zone you keep

The same swap works for records alone. Apply the renamed records first, then
strip the old records' finalizers and delete them:

```bash
kubectl apply -f renamed-records.yaml
kubectl -n "$NS" get arecords          # wait for the new ones to be Ready

OLD="arecord/example-com-www arecord/example-com-api"
for r in $OLD; do
  kubectl -n "$NS" patch "$r" --type=merge -p '{"metadata":{"finalizers":null}}'
done
kubectl -n "$NS" delete $OLD
```

The kept zone's status still lists the old resources. On its next reconcile
the zone drops them from status. It does **not** delete their DNS data from
BIND while another record the zone selects declares the same name and type.
The operator log records this as `Keeping DNS data of deleted ARecord ...:
still declared by ARecord ...`.

!!! note "Older bindy releases"
    Before this check existed, the zone's stale-record cleanup removed a
    deleted record's RRset even when a renamed record declared it. The name
    stopped resolving until the renamed record reconciled again. On those
    releases, rename the zone together with its records (previous section),
    or rename records in a quiet window.
