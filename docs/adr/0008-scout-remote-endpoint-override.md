# 0008 — Scout remote transport: endpoint override with file-based credentials

- **Status:** Accepted
- **Date:** 2026-09-28
- **Deciders:** Erick Bourgeois
- **Related:** Closes roadmap 12 Phase 3
  (`.github/community/12-scout-ingress-controller.md`); extends
  [ADR-0002](0002-scout-deployment-topology.md)'s credential-direction
  principle

## Context

Scout reaches the bindy (queenship) cluster today in exactly one remote way:
a full kubeconfig blob in a local Secret (`BINDY_SCOUT_REMOTE_SECRET`,
Phase 2). Roadmap 12's Phase 3 proposed replacing that with a Linkerd
multicluster service mirror, "using Scout's local ServiceAccount token
authenticated via Linkerd mTLS."

Two premises of that phrasing do not survive contact with how the pieces
actually work:

1. **Linkerd meshes workloads, not the control plane.** The Kubernetes API
   server runs no Linkerd sidecar, so "mTLS to the API server via the mesh"
   is only possible through a *meshed proxy* in front of the API (e.g., a
   mirrored `kube-api-proxy` Deployment on the bindy cluster). The mesh can
   secure the hop; it cannot make the API server itself a mesh participant.
2. **Transport does not solve authentication.** Whatever path the request
   takes, the bindy cluster's API server must be handed a credential *it*
   accepts. A drone cluster's local ServiceAccount token is not one — there
   is no trust federation between the clusters, and ADR-0002's credential
   direction principle (low-privilege credentials pointing AT the bindy
   cluster, never the reverse) says the credential must be minted by the
   bindy cluster regardless.

What *is* wrong with the Phase 2 mechanism is its shape, not its direction:
a kubeconfig blob bundles endpoint, CA and token into one opaque Secret,
which is awkward to rotate (the whole blob must be rewritten) and
over-privileged as a format (it can name any server).

## Decision

Add an **endpoint override** remote mode with file-based credentials, as a
peer to (not a replacement of) the kubeconfig-Secret mode:

| Variable | CLI | Meaning |
|---|---|---|
| `BINDY_SCOUT_REMOTE_ENDPOINT` | `--remote-endpoint` | Bindy cluster API URL (a Linkerd-mirrored meshed proxy, konnectivity endpoint, or the API server directly) |
| `BINDY_SCOUT_REMOTE_TOKEN_FILE` | `--remote-token-file` | Path to a bearer token minted by the **bindy** cluster (mounted Secret or CSI/external-secrets delivery); re-read on expiry by kube-rs, so rotation needs no restart |
| `BINDY_SCOUT_REMOTE_CA_FILE` | `--remote-ca-file` | Path to the endpoint's CA bundle (PEM). Optional: absent means webpki public roots |

Configuration is **fail-closed and unambiguous**: the endpoint mode requires
the token file; setting both `BINDY_SCOUT_REMOTE_ENDPOINT` and
`BINDY_SCOUT_REMOTE_SECRET` is a startup error, as is a token/CA file
without an endpoint. Selection order is explicit, never silent: endpoint
mode when the endpoint is set, kubeconfig-Secret mode when the secret is
set, same-cluster mode otherwise.

Implementation: Scout synthesizes an in-memory `Kubeconfig` (server = the
endpoint, `certificate-authority` = the CA path, user `tokenFile` = the
token path) and feeds it through the same
`Config::from_custom_kubeconfig` path Phase 2 uses — inheriting kube-rs's
PEM handling and token-file refresh instead of hand-rolling either.

The Linkerd deployment pattern this enables (mirrored meshed API proxy on
the queenship, drone-side Scout pointed at the mirror, token delivered as a
file) is documented in the Scout guide; Phase 3's original wording is
superseded by this ADR.

## Consequences

- The remote credential shrinks from a kubeconfig blob to a bare token
  file: endpoint and CA live in the Deployment spec (auditable, GitOps-
  diffable), and rotating the credential is rewriting one file — no blob
  reassembly, no pod restart.
- The mechanism is transport-agnostic: Linkerd multicluster mirrors,
  konnectivity, or a plain reachable API server all work; the mesh case gets
  its mTLS from the sidecar exactly where Linkerd provides it.
- RBAC is unchanged; `secrets: get` remains needed only for the
  kubeconfig-Secret mode. The bindy-cluster ServiceAccount and its scoped
  Role from Phase 2 are reused as-is.
- Live verification against a real Linkerd multicluster pair is deferred to
  the two-cluster staging environment (tracked with the other live-cluster
  verifications) — the code path is unit-tested to client construction.
- `bindy bootstrap scout` does not yet template the new env vars into the
  Deployment; operators set them directly. Recorded as a follow-up
  (roadmap 27).
- No CALM topology change: the scout → queen-api relationship is the same
  edge with a second transport; the relationship descriptions note it.
