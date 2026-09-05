# Remote authority exposure gate

The authenticated authority is not yet promotable beyond loopback. This gate records the exact
security properties that must be demonstrated before the `SecureNetworkExposure` type gains a
non-loopback variant. Configuration fields or operator assertions alone do not count as evidence.

## Automated attack and failure matrix

| Boundary | Adversarial or failure case | Current executable evidence | Promotion state |
|---|---|---|---|
| Socket policy | IPv4 wildcard, private IPv4, IPv6 wildcard, and IPv4-mapped IPv6 bypass attempts | `server_config::tests::unknown_fields_and_every_remote_bind_class_fail_closed` and `secure_authority_rejects_non_loopback_without_production_policy` | Pass, closed |
| Policy composition | OIDC discovery and TLS reload are both present but no reviewed remote policy exists | `server_config::tests::lifecycle_workers_cannot_implicitly_unlock_remote_exposure` | Pass, closed |
| Trust files | Direct final-component symbolic link and final-component link-swap target | `server_config::tests::kernel_no_follow_open_rejects_symbolic_links`; Unix opens use kernel `O_NOFOLLOW` and validate the opened descriptor | Pass |
| TLS peer identity | Client does not trust the presented server certificate | `untrusted_server_certificate_fails_before_application_admission` | Pass |
| TLS lifecycle | Unchanged identity check, replacement mismatch, then coherent rotation during an active session | `tls_reload_keeps_the_previous_deadline_until_a_valid_pair_is_installed`, `tls_rotation_changes_future_handshakes_without_disrupting_an_active_session`, and the unchanged/rotation standalone process tests | Pass |
| Application admission | Invalid credential, stalled hello, or nonce mismatch | Secure transport and standalone process negative tests | Pass |
| OIDC provenance | Discovery issuer mismatch, unsafe endpoint, malformed or stale key set, and replay | OIDC discovery/verifier tests and standalone discovery refusal | Pass |
| Gameplay authorization | Invalid session and malformed or replayed command | Network, transport, and secure authority tests | Pass |
| Resource abuse | Per-session datagram burst, queue pressure, pending handshake cap, and authority capacity | Secure authority rate-limit test plus bounded constants and unit tests | Partial: add concurrent hostile-process load |
| Loss and reordering | Distinct delay, duplication, and loss traces with bounded repair | Four-client real-process trace replay | Pass for deterministic loopback; add WAN profiles |
| Trust outage | Discovery or renewal repeatedly fails until monotonic expiry | Refresh controller tests and deadline shutdown logic | Partial: add process-level expiry outage cases |
| Platform ACL | Secret ownership and permissions are installer-owned on every supported server OS | Unix non-root service identity, owner, mode, and no-follow tests | Partial: parent-directory policy and Windows service DACL validation missing |

Every executable row is part of the normal test suite; no network namespace, firewall exception, or
remote bind is needed to rehearse it. A failure in any row blocks promotion.

## Remaining promotion proofs

1. A provisioner renews a CA-issued server identity atomically, the authority reloads it, and a
   forced provisioner outage makes the process stop at the monotonic certificate deadline.
2. The production OIDC issuer and root are provisioned outside the repository; startup discovery,
   key rotation, stale-key expiry, and issuer mismatch are exercised against a disposable realm.
3. The Linux service account and Windows service DACL are checked from the opened handles, including
   owner, inheritance, write access, links/reparse points, and parent-directory replacement rights.
4. Hostile external clients exercise handshake saturation, malformed datagrams, replay, burst rate,
   queue pressure, packet loss, duplication, reordering, and reconnect storms without exceeding the
   fixed simulation budget or leaking credentials.
5. A reviewed deployment policy names the exact interface, UDP port, certificate name, identity
   issuer, firewall source ranges, observability budget, rollback owner, and expiry. Wildcard binds
   remain forbidden for the first private-network demo.

Only after all five proofs are reproducible may the internal validated binder gain a private-network
capability. Internet publication remains a separate later gate with capacity protection and incident
response evidence.
