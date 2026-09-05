# Secure dedicated server

`secure-dedicated-server` is the authenticated QUIC authority entry point. It is currently a
loopback deployment milestone: configuration, OIDC verification, transport admission, gameplay
commands, simulation, and shutdown run end to end, while remote exposure remains deliberately
disabled.

## Configuration contract

The process accepts only `--config <absolute-json-path>`. Tokens, private-key bytes, and client
secrets are never command-line options. The JSON document is limited to 16 KiB and rejects unknown
fields.

| Field | Contract |
|---|---|
| `bind` | Loopback socket address. Port zero is allowed only when `max_ticks` bounds the run. |
| `exposure` | Must be `loopback`. No remote value exists yet. |
| `certificate_chain_file` | Absolute, regular, non-link PEM file; at most 256 KiB and eight currently valid certificates, each with more than 60 seconds remaining. |
| `private_key_file` | Absolute, regular, non-link PEM file; at most 64 KiB and exactly one private key. |
| `oidc_jwks_file` | Absolute, regular, non-link JWKS file; at most 64 KiB and 32 validated RS256 keys. |
| `oidc_issuer` | Exact HTTPS token issuer. |
| `oidc_audience` | Exact game audience. |
| `jwks_valid_until_unix_seconds` | Absolute expiry with more than 60 and at most 86,400 seconds remaining at startup; the final minute is reserved. |
| `oidc_discovery` | Optional object enabling mandatory startup discovery and periodic refresh. |
| `oidc_discovery.refresh_interval_seconds` | Whole seconds from 60 through 3,600. Refreshed keys remain usable for at most three configured intervals without another successful refresh. |
| `oidc_discovery.root_certificate_file` | Optional absolute, regular, non-link PEM trust bundle; at most 256 KiB and 16 certificates. Platform roots remain available. |
| `tls_reload` | Optional object enabling periodic reload of the configured certificate/key paths for future handshakes. |
| `tls_reload.interval_seconds` | Whole seconds from 5 through 3,600. The installed chain must remain valid beyond this interval plus the 60-second safety margin. |
| `max_ticks` | Optional positive fixed-tick limit; intended for bounded validation runs. |
| `stop_after_commands` | Optional positive applied-command limit; intended for bounded validation runs. |

On Unix, the standalone authority refuses effective UID 0. The configuration, certificate, JWKS,
and optional discovery root bundle must be owned by either root or the service UID and must not be
group/world writable. The private key must be owned exactly by the service UID with no group/world
permissions, normally mode `0600`. Every file is opened only after link and regular-file type
checks. Its immediate parent must be a non-link directory owned by root or the service UID and not
group/world writable. The opened file is then read through a second size bound. On Windows the current
standard-library implementation cannot validate DACL ownership; the loopback restriction remains a
mandatory boundary until an installer-owned service ACL check is implemented.

The PEM buffer holding the private key is zeroized after parsing. Rustls checks that the leaf
certificate and private key are compatible. Every certificate in the chain is parsed independently,
must be currently valid, and must have more than 60 seconds remaining. Startup reserves the final
minute of both the earliest certificate lifetime and the static JWKS validity window, then converts
those earlier safety points into monotonic deadlines, so a wall-clock rollback cannot extend either.
The verifier rejects new admissions at its OIDC trust deadline, and the process terminates on the
first tick at or after either safety deadline.

The static JWKS remains a bounded bootstrap and rollback input. When `oidc_discovery` is configured,
the process must fetch and validate discovery metadata plus a complete JWKS before emitting `READY`.
It then refreshes outside the simulation loop at the configured interval. Each successful complete-set
swap advances a monotonic deadline by three intervals; any failed fetch keeps the previous keys and
deadline, emits only a non-secret failure counter, and ultimately stops the authority rather than
serving indefinitely stale identity data. HTTPS requires TLS 1.2 or later, exact issuer equality,
same-origin JWKS, no redirects, no ambient proxy, three-second connect and five-second total
deadlines, and 16 KiB/64 KiB response limits.

When `tls_reload` is configured, an off-tick worker rereads the same fixed certificate and private-key
paths. It repeats type, permission, size, chain-lifetime, cardinality, and key-pair validation before
installing the complete replacement for future QUIC handshakes. A partial or incoherent external
file update leaves the previous endpoint configuration and monotonic deadline in force. Existing
connections retain their negotiated identity and are not interrupted. A SHA-256 fingerprint is
computed incrementally from the public DER chain only; private-key bytes are never hashed or logged.
An unchanged validated chain retains the exact previous deadline and does not update the endpoint.
The worker records non-secret attempt/success/failure plus installed/unchanged counters, so a stalled
external provisioner is observable; repeated failure cannot extend the old certificate deadline. Its
wall-clock input is floored by elapsed monotonic time from startup, so rereading an unchanged file
after a system-clock rollback cannot manufacture extra validity. An external ACME or internal-PKI
provisioner must still renew and atomically replace the files.

## Local launch

Copy [`../config/secure-server.example.json`](../config/secure-server.example.json) outside the
repository, replace every path and the JWKS validity deadline, provision the key with operating-system
secret controls, and launch:

```bash
cargo run --release --bin secure-dedicated-server -- \
  --config /absolute/path/to/secure-server.json
```

Connect the graphical client with the certificate name, trusted root, and an owner-only credential
file provisioned for that authority:

```bash
cargo run --release --bin multiplayer-demo -- \
  --secure-server 127.0.0.1:40001 \
  --server-name game.local \
  --ca-cert /absolute/path/to/ca.pem \
  --credential-file /absolute/path/to/access-token-player-1
```

The client refuses partial secure configuration and refuses mixing this mode with the legacy
`--server` option. Its bounded asynchronous receive queue keeps QUIC work off the window thread and
reports local overflow explicitly.

For local reproduction only, `cargo run --example secure_local_fixture -- <new-absolute-directory>`
generates an owner-only, short-lived fixture for `127.0.0.1:40001` and TLS name `localhost`. It never
prints the generated key or either distinct player credential. Start the server plus one or two
clients with the paths it reports, then delete the complete directory. This convenience authority is
deliberately unsuitable for LAN or Internet exposure.

Readiness emits only the selected socket, non-secret exposure class, and whether OIDC refresh and TLS
reload are active. The final line contains bounded gameplay and lifecycle counters, never
credentials, remote endpoints, or principal identifiers. It explicitly reports refused connections,
handshake/admission failures, gameplay-queue drops, protocol rejections, and rate-limited sessions.
`Ctrl-C`, certificate/JWKS safety
deadlines, `max_ticks`, and `stop_after_commands` all converge through endpoint shutdown.

Do not expose this milestone to a LAN or the Internet. Remote enablement still requires production
issuer/root provisioning, automated certificate issuance, platform secret-ACL checks, external
loss/abuse tests, and an explicit reviewed exposure policy. The executable cases, remaining proofs,
and promotion rule are tracked in [`remote-exposure-gate.md`](remote-exposure-gate.md).
