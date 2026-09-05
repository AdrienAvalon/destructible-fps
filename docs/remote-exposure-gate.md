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
| TLS lifecycle | Unchanged identity check, replacement mismatch, coherent rotation during an active session, then renewal outage | `tls_reload_keeps_the_previous_deadline_until_a_valid_pair_is_installed`, `tls_rotation_changes_future_handshakes_without_disrupting_an_active_session`, and all three lifecycle process tests | Pass |
| Application admission | Invalid credential, stalled hello, or nonce mismatch | Secure transport and standalone process negative tests | Pass |
| OIDC provenance | Discovery issuer mismatch, unsafe endpoint, malformed or stale key set, and replay | OIDC discovery/verifier tests and standalone discovery refusal | Pass |
| Gameplay authorization | Invalid session and malformed or replayed command | Network, transport, and secure authority tests | Pass |
| Resource abuse | Per-session datagram burst, queue pressure, pending handshake cap, authority capacity, and reconnect cycling | External-process 32-plus-one saturation, oversized-datagram closure, 16-session queue pressure, 32 authenticated reconnects, secure-authority rate limiting, and bounded unit tests | Pass for deterministic loopback processes; replay through the reviewed LAN profile before exposure |
| Loss and reordering | Distinct delay, duplication, and loss traces with bounded repair | Four-client real-process trace replay | Pass for deterministic loopback; add WAN profiles |
| Trust outage | Discovery, static trust, or renewal reaches its monotonic deadline | Real-process TLS outage and static OIDC expiry tests pass; discovery retains failed-refresh controller tests | Pass for local mechanisms; repeat live discovery outage against the disposable realm |
| Platform ACL | Secret ownership and permissions are installer-owned on every supported server OS | Unix non-root service identity plus file/parent owner, mode, type, and no-follow tests | Partial: Windows service DACL validation missing |
| Deployment policy | Exact interface/address/port, certificate name, issuer, source ranges, budgets, owner, and expiry | Bounded `LanDeploymentPolicy`, read-only exact host-assignment attestation, offline exact-SAN/ordered-chain/single-reviewed-CA attestation, negative matrix, and `lan-policy-check` | Partial: instantiate and review; installed key, port/firewall/OIDC/ACL proofs remain |

Every executable row is part of the normal test suite; no network namespace, firewall exception, or
remote bind is needed to rehearse it. A failure in any row blocks promotion.

## Remaining promotion proofs

1. A provisioner renews a CA-issued server identity atomically and the authority reloads it. The
   forced provisioner-outage shutdown is now proven independently against the real process.
2. The production OIDC issuer and root are provisioned outside the repository; startup discovery,
   key rotation, stale-key expiry, and issuer mismatch are exercised against a disposable realm.
3. The Linux service account and Windows service DACL are checked from the opened handles, including
   owner, inheritance, write access, links/reparse points, and parent-directory replacement rights.
4. Hostile external clients exercise handshake saturation, malformed datagrams, replay, burst rate,
   queue pressure, packet loss, duplication, reordering, and reconnect storms without exceeding the
   fixed simulation budget or leaking credentials.
5. Instantiate and review the bounded deployment-policy contract for the target host, repeat its
   host and certificate attestations against installed material, then prove the private key matches,
   exact UDP port, identity issuer, firewall source ranges, observability budget, rollback owner, and
   expiry against live state. Wildcard binds remain forbidden for the first private-network demo.

The in-process hostile client rehearsal now establishes 32 concurrent trusted TLS connections while
withholding every application hello. The 33rd connection is refused within a fixed deadline, the
counter reports exactly one refusal, no session becomes authoritative, and repeated authority ticks
perform no gameplay work. This covers the pending-admission cap through real sockets; it does not
replace the remaining separate-process reconnect, malformed-input, and queue-pressure campaign.

The TLS outage rehearsal uses a short-lived but initially admissible identity, then corrupts the
renewal input after readiness. Every watcher attempt fails, no candidate is installed, and the real
process exits at the monotonic certificate safety deadline, which reserves the final 60 seconds of
X.509 validity. Automated CA issuance itself remains outside this repository-level rehearsal.

The static OIDC rehearsal gives the bootstrap JWKS 66 seconds of declared validity and proves that
the real process reserves the final minute, emits no fictitious refresh activity, and exits at the
resulting monotonic trust deadline. A live failed-discovery expiry still belongs to the disposable
production-shaped realm campaign rather than this static-file proof.

The hostile-load test now crosses a real process boundary: 32 external clients hold completed TLS
handshakes without application hellos, the 33rd is refused, and the process completes 240 ticks with
zero admissions or simulation traffic. A separate authenticated client sends a 1,101-byte QUIC
datagram; the server closes it, reports exactly one protocol rejection, and forwards nothing to the
authority. The reconnect and multi-session queue-pressure campaigns complete the local matrix below.

The remaining two local hostile-load cases fill all 16 authority slots and concurrently offer up to
240 maximum-size malformed datagrams per session, then execute 32 complete authenticated reconnects.
Queue memory stays at the fixed 256-event ceiling, abusive sessions close after 32 consecutive drops,
queued data is rejected or counted malformed without applying a command, and every reconnect leaves
matching admission/disconnection totals with no live session. The same campaign must still be replayed
through the reviewed LAN interface and firewall profile.

Only after all five proofs are reproducible may the internal validated binder gain a private-network
capability. Internet publication remains a separate later gate with capacity protection and incident
response evidence.

## Offline deployment-policy contract

`config/lan-deployment-policy.example.json` is a non-secret template, not an approved policy. The
`lan-policy-check` binary reads at most 16 KiB, rejects unknown fields, and accepts only a canonical
schema-v1 document that expires in more than 60 seconds and no more than 24 hours. The source CIDRs
must be private, canonical, non-overlapping, and in the same address family as the exact bind address.
The declared authority limits must exactly match the compiled player, pending-handshake, gameplay
queue, and per-session datagram ceilings; a document cannot raise them.

The checker performs no socket operation and never mutates a network interface, firewall,
certificate store, identity provider, or service manager. Offline mode does not inspect them. With
`--verify-host`, it enumerates at most 256 interface-address records and requires exactly the named
operational interface, address, non-zero index and fully private prefix. It rejects the same address
under any other interface name and never echoes topology or identity values. Paired
`--certificate-chain` and `--trust-anchor` options read only public, integrity-protected files. They
require one literal policy SAN, explicit server-auth usage, one ordered eight-entry-maximum chain,
one reviewed self-issued CA, and cryptographic validity now and through policy expiry plus the
60-second safety margin. No private key, DNS request, socket or revocation service is involved.

An offline `POLICY_OK` means only that a proposal is bounded and unambiguous; a host-verified result
adds only a point-in-time interface assignment, while a certificate-verified result adds only a
point-in-time public chain proof. None makes the proposal approved, proves installed private-key
correspondence or the remaining target state, or unlocks the loopback-only server. A future launcher
must repeat both proofs immediately before bind and fail closed if subsequent interface, certificate
or policy monitoring reports drift.
