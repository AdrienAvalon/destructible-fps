# Private-LAN deployment policy contract

The schema-v1 policy is an offline review artifact for the first multi-machine demonstration. It is
not consumed by `secure-dedicated-server`, and the public server API still refuses every non-loopback
address.

## Workflow

1. Copy `config/lan-deployment-policy.example.json` outside this repository.
2. Replace the deployment identifier, exact OS interface name, private address and UDP port.
3. Set the exact certificate DNS name and OIDC issuer used by the proposed authority.
4. List only canonical, non-overlapping client source CIDRs in the bind address family.
5. Name an accountable rollback owner and set an expiry between 61 seconds and 24 hours ahead.
6. Keep every runtime limit equal to the compiled authority limit and keep observability within the
   schema ceilings.
7. Run:

```bash
cargo run --release --bin lan-policy-check -- /absolute/path/to/lan-policy.json
cargo run --release --bin lan-policy-check -- --verify-host /absolute/path/to/lan-policy.json
cargo run --release --bin lan-policy-check -- \
  --certificate-chain /absolute/path/to/server-chain.pem \
  --trust-anchor /absolute/path/to/reviewed-root.pem \
  /absolute/path/to/lan-policy.json
cargo run --release --bin lan-policy-check -- \
  --verify-host \
  --certificate-chain /absolute/path/to/server-chain.pem \
  --trust-anchor /absolute/path/to/reviewed-root.pem \
  /absolute/path/to/lan-policy.json
```

The first command validates only the document. The second additionally takes one read-only host
interface snapshot and proves the exact assignment. The certificate options are inseparable and
accept only absolute paths to one ordered leaf/intermediate chain and one explicitly reviewed
self-issued CA. They can be combined with host verification. The checker prints only the schema
version, source-range count, remaining lifetime, and proof booleans. It does not echo topology,
certificate names, paths, fingerprints, identities, or owner fields.

## Offline certificate evidence

Certificate inputs are public but integrity-sensitive. Each file is capped at 256 KiB; the presented
chain is capped at eight entries and the reviewed trust file must contain exactly one CA. Canonical
PEM permits certificate sections and blank separator lines only. Duplicate entries, an embedded
anchor, unrelated or out-of-order intermediates, non-CA issuers, and malformed X.509 fail closed.
On Unix the checker refuses a final-component symlink, opens with `O_NOFOLLOW`, and requires both the
file and its immediate parent to be owned by root or the current user and not group/world writable.
Windows DACL and reparse-point evidence remains a separate promotion gate.

The leaf must contain exactly one SAN entry: the policy's literal lowercase DNS name. Wildcards,
aliases, IP SANs, CN fallback, CA leaves and certificates without explicit server-auth usage are
rejected. Rustls/webpki validates the chain and DNS identity against only the supplied anchor at the
current instant and again at policy expiry plus 60 seconds. Independent X.509 checks require the
leaf, every intermediate and the trust anchor to cover both instants, including the final margin.
The returned proof binds the material with length-prefixed SHA-256 fingerprints, but the CLI does
not print them.

This is a point-in-time offline chain proof. It does not read or validate a private key, contact a
CA, fetch OCSP/CRL data, resolve DNS, or inspect the certificate installed in a running service.
Automated issuance, atomic installation and renewal/revocation policy remain required before LAN
promotion.

## Fail-closed schema

| Field | Contract |
|---|---|
| `schema_version` | Exactly `1`; unknown fields are rejected at every object level. |
| `deployment_id` | Non-empty bounded ASCII identifier. |
| `interface` | One bounded exact UTF-8 name; control characters, edge whitespace, and wildcard aliases are rejected. Interior spaces are permitted for Windows friendly names. |
| `bind_address` | One RFC1918 IPv4 or IPv6 ULA address; wildcard, loopback, public, mapped and unspecified forms are rejected. |
| `udp_port` | One non-zero port. |
| `certificate_dns_name` | Lowercase multi-label DNS name without wildcard or IP literal. |
| `oidc_issuer` | Canonical HTTPS URL without credentials, query, or fragment. |
| `firewall_source_cidrs` | One to 16 canonical, private, same-family and non-overlapping ranges. |
| `runtime_limits` | Exact equality with compiled player, handshake, queue and datagram ceilings. |
| `observability` | Non-zero status/log/metric budgets under fixed schema maxima. |
| `rollback_owner` | Non-empty bounded ASCII operator or team identifier. |
| `expires_at_unix_seconds` | More than 60 seconds and at most 24 hours after validation. |

## Promotion boundary

Offline validity is necessary but insufficient. The future launcher must securely open this file,
convert expiry to a monotonic deadline, and independently prove all of the following without trusting
textual assertions:

- the named interface exists and owns exactly the declared address;
- the socket is bound only to that address and port, never a wildcard or dual-stack alias;
- the installed certificate repeats the offline exact-name/chain proof against the reviewed CA;
- discovery returns the exact issuer and reviewed trust root;
- the active host firewall permits only the declared source ranges;
- service ACLs and secret-file ownership pass on the target operating system;
- telemetry and automatic shutdown/rollback remain observable for the full window.

Only a separate, later reviewed capability may combine those proofs with a non-loopback binder.

## Host inventory dependency

Host verification uses the pinned `if-addrs` 0.14.0 crate through its safe public API. Its sole
runtime role is a synchronous, point-in-time enumeration requested explicitly by the checker; it is
never called from the simulation, render or packet path. The crate is about 64 KiB of source,
MIT/BSD-3-Clause licensed, supports POSIX and Windows, and brings only the platform FFI layer
(`libc` or `windows-sys`). This replaces shelling out to `ip`, `ifconfig` or PowerShell and removes
command resolution, localized output and shell-injection concerns. The dependency remains exactly
pinned and must be tested on native Windows and macOS runners before distribution promotion.
