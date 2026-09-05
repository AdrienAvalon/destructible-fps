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
| `certificate_chain_file` | Absolute, regular, non-link PEM file; at most 256 KiB and eight certificates. |
| `private_key_file` | Absolute, regular, non-link PEM file; at most 64 KiB and exactly one private key. |
| `oidc_jwks_file` | Absolute, regular, non-link JWKS file; at most 64 KiB and 32 validated RS256 keys. |
| `oidc_issuer` | Exact HTTPS token issuer. |
| `oidc_audience` | Exact game audience. |
| `jwks_valid_until_unix_seconds` | Absolute expiry with 60 to 86,400 seconds remaining at startup. |
| `max_ticks` | Optional positive fixed-tick limit; intended for bounded validation runs. |
| `stop_after_commands` | Optional positive applied-command limit; intended for bounded validation runs. |

On Unix, the configuration, certificate, and JWKS must not be group/world writable. The private key
must have no group/world permissions, normally mode `0600`. All four files are opened only after
links and regular-file type checks, then read through a second size bound. On Windows the current
standard-library implementation cannot validate DACL ownership; the loopback restriction remains a
mandatory boundary until an installer-owned service ACL check is implemented.

The PEM buffer holding the private key is zeroized after parsing. Rustls checks that the leaf
certificate and private key are compatible. Startup converts the absolute JWKS deadline into one
monotonic deadline, so a wall-clock rollback cannot extend it. The verifier rejects new admissions at
expiry, and the process terminates on the first tick at or after that deadline. Set a short rotation
horizon and replace the complete file/config pair atomically; automatic trusted discovery and refresh
are a later gate.

## Local launch

Copy [`../config/secure-server.example.json`](../config/secure-server.example.json) outside the
repository, replace every path and the JWKS validity deadline, provision the key with operating-system
secret controls, and launch:

```bash
cargo run --release --bin secure-dedicated-server -- \
  --config /absolute/path/to/secure-server.json
```

Readiness emits only the selected socket and the non-secret exposure class. The final line contains
bounded counters, never credentials or principal identifiers. `Ctrl-C`, `max_ticks`, JWKS expiry,
and `stop_after_commands` all converge through endpoint shutdown.

Do not expose this milestone to a LAN or the Internet. Remote enablement requires trusted discovery
and key refresh, certificate validity/lifecycle enforcement, platform secret-ACL checks, external
loss/abuse tests, and an explicit reviewed exposure policy.
