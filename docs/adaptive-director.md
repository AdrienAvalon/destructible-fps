# Optional adaptive game director and interchangeable AI backends

Status: **planned; no live AI director, NPC model integration or provider adapter is delivered**.
This extends the FPS with player-enabled adaptive encounters, dialogue, scenario branches and
world events. It does not replace the current visual, physical or multiplayer acceptance gates.
The game must remain playable with all model inference disabled.

## What can adapt

A director can propose pacing, encounters, objectives and dialogue based on permitted match state
and explicit player preferences. An NPC can choose a legal goal or speak within its role and
perception. A world event can request a supported physical change or a prevalidated content variant.
None of these may fabricate physics, change weapon rules or modify arbitrary live geometry.

Example acceptance scene: players breach a supported wall; the authoritative event opens a route;
an allied NPC requests that route after navigation is revalidated; the director offers an optional
objective compatible with the changed map. Clients and late joins receive the same accepted event
sequence. If inference is absent or late, a deterministic authored branch provides a playable route.
This scenario depends on the still-planned NPC/scenario systems in
[`gameplay-authoring.md`](gameplay-authoring.md).

Generate substantial new maps, meshes, textures and navigation data outside the live simulation,
then validate and cook them as candidate content. Loading a new region requires compatible client
packages, collision/support/traversal checks, a safe transition and explicit size/time budgets.
Small live world changes use the existing authoritative transaction semantics; no walls appear
inside players, no unavoidable collapse is inserted under them, and no client-only event is treated
as shared gameplay. Novelty starts with recombination of validated content, not arbitrary new code.
Loading newly generated regions during a match is excluded from the first AI-01 slice. Promote it
only through an additional content-transition fixture with active players, missing/incompatible
packages, interrupted loads, late join and checkpoint restoration. Until then, use the loaded map
and its validated scenario capabilities; do not advertise live region generation.

## Authority and information flow

The planned path is:

1. The server exports a bounded, purpose-filtered observation at a known world/scenario revision.
2. A separate asynchronous coordinator obtains an untrusted typed proposal from an eligible backend.
3. Deterministic server policy checks principal/capability, match mode, action schema, scope, target
   existence, current preconditions, expiry, cooldown, fairness and resource budgets.
4. The server atomically records an accepted event and applies it at a defined simulation tick,
   using the same physical/gameplay systems as authored scenarios. Rejected proposals change nothing.
5. Ordinary replication, persistence and diagnostic receipts carry the accepted outcome.

The integration point is the bounded tick orchestration in
[`AuthorityCore`](../src/network.rs), not its client datagram decoder. Add a server-issued,
match-bound director capability and a distinct bounded proposal inbox; never impersonate a player,
mint a wire session from model output or bypass ordinary gameplay/physics mutation validation.
The coordinator receives only a revocable capability handle, not authority to choose its principal
or scope. Tick admission revalidates the action and consumes the same aggregate simulation budget
as other work. This hook and its inbox are not implemented today.

Policy is not another model's approval. Capabilities come from the server, never a prompt, model
output or player name. Dialogue, chat, map text and reference images remain untrusted data; even
valid JSON cannot authorize an action. The runtime interface has no shell, filesystem, external URL
fetch, code execution, admin command or developer-tool access. Observation builders enforce both
match isolation and NPC knowledge limits, including hidden enemy positions and unrevealed objectives.

Adaptive solo/co-op is opt-in; players can disable it or constrain permitted novelty/difficulty.
Use declared preferences and bounded in-match signals, not inferred sensitive traits, emotional
vulnerability or a cross-game personal profile. Do not optimize for spending or compulsory return.
Competitive/ranked rules default to a fixed, disclosed director policy without individualized
advantages; open-ended narrative adaptation is for explicitly selected compatible modes.

In shared matches, enable world-affecting adaptation only after every participant accepts the same
disclosed policy. A non-consenting late join or consent withdrawal suspends adaptive proposals and
uses the authored fallback without undoing accepted physical history. Fence outstanding requests
with a server-owned policy epoch so late answers cannot re-enable adaptation. Never send a
non-consenting participant's state to a model; if redaction makes an observation unusable, do not
infer the missing state. Optional private dialogue has separate participant-scoped consent and
must not import another participant's chat, profile or hidden state.

## Backend-neutral contract

The game depends on an internal observation/proposal contract, not a model name, tokenizer or vendor
message format. A backend adapter translates that contract and reports its supported capabilities.
Changing the backend must not change authority, weapons, physical constants or allowed actions.

Planned backend classes are a deterministic no-model director for fallback/testing, a local model
server, and explicitly enabled remote APIs. A production runtime is a separately managed game
component: the current Codex conversation is not a permanent in-game process. A development agent
may participate through the same restricted interface during an owned test session.

Local runners such as Ollama and llama.cpp offer OpenAI-compatible interfaces, but compatibility
does not establish identical endpoints, tool behavior or model quality. Ollama documents partial
API compatibility; llama.cpp documents constrained JSON and tool calling. These are adapter
candidates, not installed integrations or a selected model.
[Ollama documentation](https://docs.ollama.com/api/openai-compatibility) and
[llama.cpp server documentation](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md),
checked on 2026-09-06.

Each adapter must negotiate or probe schema version, supported modalities, context/output bounds,
structured proposal support, cancellation, concurrency and usage reporting. A text-only model may
serve dialogue but cannot analyze a reference photograph. Models without reliable constrained
output cannot issue gameplay proposals until an adapter passes validation; parsing free text as a
command is forbidden. Missing capability selects an explicitly configured compatible fallback or
reports unsupported. Never silently downgrade validation. "Any model" means any model with a
suitable tested adapter and deployment license, not arbitrary weights with universal capabilities.

Record exact runtime/model revision or local weights digest, quantization and relevant generation
settings in test receipts. Review code and weights provenance/licensing separately: local or
open-weight does not necessarily mean open-source or freely redistributable. Model replacement
requires the same conformance fixtures; no automatic download, provider switch or version upgrade
during a match. A configured hot switch cancels/fences the old generation, rejects its late answers
and starts from current match state. It does not erase prior accepted events.

## Latency, compute, cost and privacy

- No model call runs in a render, physics or network-receive loop. A low-frequency, event-driven
  director and optional contextual NPC dialogue sit above ordinary fixed-step NPC behavior.
- Bound pending requests, input/output bytes and tokens, history, per-match/actor rate, deadlines,
  accepted actions and retries. Reserve cost before dispatch across concurrent requests; an API
  with no enforceable configured cost bound is not enabled. Timeout, invalid output, exhausted
  quota or provider failure triggers a deterministic fallback, never an unbounded repair loop.
- Local inference avoids per-call API billing, not hardware/electricity costs. Measure shared-GPU
  frame-time tails, VRAM/RAM and server tick tails under simultaneous combat and inference. Prefer
  bounded inference workloads or a separately provisioned machine when contention exceeds the
  game's budget. A remote LAN host is a separate authenticated deployment, not implicitly trusted.
- A local-only policy blocks external inference egress and fails locally; it never falls back to
  paid cloud inference. Configure endpoints out of band, reject redirects to another origin, keep
  credentials server-side, and never enable a model runner's optional built-in host tools.
- Cloud inference requires explicit opt-in, data disclosure and a configured spending ceiling;
  no service is bought or enabled by this design. Minimize exported state. Voice/chat transmission
  and any persistent personal preference storage require separate explicit consent and retention
  rules. A player disabling adaptation stops new collection and pending personalization requests.

Initial conformance profile (candidate limits, not measured production capacity): one in-flight
request per match, four pending proposals, at most one dispatch every five seconds, a five-second
request deadline, 16 KiB/2,048-token input and 8 KiB/256-token output ceilings, and at most one
accepted director event per tick under the existing aggregate simulation budget. Enforce both
byte and token bounds; reject adapters that cannot enforce the configured token/output contract.
The first offline fixture permits at most eight calls and zero paid API spend. Overflow/expiry
selects fallback. Production profiles must additionally declare measured resident RAM/VRAM limits
and host-wide concurrency reservations; an absent or exhausted resource profile refuses activation.
All profiles retain the frame/tick targets in the roadmap's
[non-negotiable budgets](production-roadmap.md#non-negotiable-budgets), measured with simultaneous
combat and inference. These limits are fixture policy, not a claim of an optimal model configuration.

The first slice retains no raw prompts/chat/voice in diagnostic logs and no personal preference
profile across sessions. Drop transient observation/response buffers at completion, cancellation or
timeout, and drop participant-scoped preferences on departure. Retain accepted gameplay events only
under an explicit bounded save/replay policy, independently of personalization. Future persistent
personalization requires a configured expiry and tested deletion of primary data and derived caches,
with backup expiry disclosed. Cloud data already transmitted cannot be recalled by cancelling a
request; private voice/chat export stays disabled until provider retention/deletion behavior is
verified and accepted. Do not promise deletion the provider cannot support.

## Persistence, replay and delivery gates

Use match-bound monotonic event IDs, preconditions, model-generation fencing and expiry. Duplicate
delivery must not spawn two encounters or award an objective twice. A durable publication boundary
must connect accepted events with their world/scenario transaction; crash recovery cannot reapply
a reward or acknowledge a decision whose effect is missing. Test stale responses, restart and
conflicting proposals alongside ordinary replication repair.

Gameplay replay consumes accepted typed decisions at recorded ticks, inputs, package/recipe identity
and the initial authoritative checkpoint. It does **not** ask a model to regenerate the same text
or decision from a seed. Store only the permitted rendered dialogue/necessary event data, with
private diagnostics separated and retention bounded. Save enough attribution for debugging without
requiring raw prompts, private chat or hidden model reasoning. Visual/performance replay remains
hardware-dependent and separate from canonical gameplay-state comparison.

Delivery order:

1. Establish scenario/NPC capabilities and a deterministic director fixture with no model dependency.
2. Implement and negatively test observation filtering, proposal validation, budgets and recorded
   decision replay, including two-client/late-join/save-restore convergence.
3. Connect one local backend in an owned test session; exercise malformed/oversized responses,
   stalled inference, cancellation, injected player instructions, disallowed actions and exhausted
   compute budgets. Repeat the same conformance fixture with a second backend to prove interchange.
4. Validate native play under concurrent destruction and inference, offline/provider-down fallback,
   explicit disable, participant data isolation and safe content transitions.
5. Offer remote backends only after the same tests plus egress, consent and hard spend controls.

This is separate from the [development agent tools](agent-tooling.md). A tool connection alone is
neither delivered adaptive gameplay nor evidence that photorealism/performance targets are met.
