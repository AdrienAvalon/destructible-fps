# Agent-callable game production tools

Status: architecture and acceptance contract, **not an installed MCP server or a delivered editor**.
The existing finite tool launchers remain usable through the terminal. This contract makes future
map/scenario creation, playtesting, inspection, profiling and regression replay callable by a human
or a development agent through the same versioned operations.

## Two distinct interfaces

Development tools operate on an explicitly selected project workspace and disposable test sessions.
The [adaptive game director](adaptive-director.md) operates on an explicitly authorized match, with
far fewer capabilities. Never give an in-game NPC the developer's terminal, repository writer,
capture filesystem, Codex permissions or infrastructure access. Player dialogue cannot switch modes.

The proposed production interface is a structured CLI/local API first, with a thin optional MCP
adapter. Do not duplicate validation or implement gameplay in the adapter. Local Codex clients
support local-process STDIO MCP servers and project-scoped configuration for trusted projects;
hosted ChatGPT web does not read that local configuration. Verify the actual client's discovered
tools before declaring a connection usable. This feasibility is documented by
[OpenAI's MCP guide](https://learn.chatgpt.com/docs/extend/mcp), checked on 2026-09-06.
It does not establish that this game has such a connection today. No Codex rights, reasoning
settings, host services or existing integrations are changed by this document.

## Proposed operation families

These are capability names for the future interface, not executable commands today.

| Operation family | Input and effect | Required result |
| --- | --- | --- |
| Discover/inspect | Query supported versions, operations, current job or test-session state | Bounded structured metadata with no side effects |
| Create/validate content | Metric recipe, seed, authored entities/scenario and declared budgets | New candidate package and reference, topology, traversal and budget checks; never overwrite the active world |
| Run/play | Validated package, implemented scenario and bounded input trace | Owned test session, explicit readiness and completion, exit reason and authoritative state identity |
| Capture/profile | Owned session, camera/region and finite capture window | Actual native image/trace plus settings, hardware, executable identity and instrumentation flags |
| Replay/compare | Recorded inputs, accepted events, package/build identities and comparison policy | State differences and like-for-like performance/image evidence, with unsupported comparisons reported |
| Cancel/status/artifacts | Opaque job ID within the caller's scope | Current phase, cancellation acknowledgement and scoped artifact manifest |

## Job, artifact and publication contract

- Version schemas and advertise only implemented capabilities. Validate bytes, nesting, counts,
  coordinates, enum values and budgets before allocation or spawning a process. Unknown actions or
  unsupported schema versions fail explicitly; no shell command or arbitrary path field is accepted.
- Server-issued scope binds each job to its workspace/session and capability set. Caller-chosen
  IDs are not authentication. Deny path traversal, symlink escapes and cross-session artifact reads;
  artifact lookup uses scoped IDs, not untrusted filenames. Do not inherit ambient service secrets.
- Long operations return an owned job ID and support bounded polling and cancellation. Enforce
  independent wall-clock, CPU/GPU, concurrency, memory, output and retained-artifact budgets. The
  simulation and presentation threads never wait for a tool call or model response.
- Use explicit states: queued, running, succeeded, failed, cancelled, timed-out or unsupported.
  A missing display, empty capture, skipped test or clean subprocess exit without evidence is not
  success. Cancellation terminates only owned work and discards unpublished candidates; cancelling
  after a completed publication does not silently undo an acknowledged result.
- Every result names build/executable and content hashes, recipe/seed, operation/schema version,
  relevant settings, validation results and bounded diagnostics. A hash identifies bytes, not their
  author or safety. Raw private player dialogue and credentials do not belong in artifacts.
- Write new candidates privately, validate them, then publish atomically after rechecking the
  expected source revision and authority. Concurrent stale candidates fail without overwriting work.
  Preserve the previous valid package and a documented restore path; no implicit deletion or push.
- Retry with idempotency keys bound to scope and input digest. The same key with different input
  fails. A disconnected caller can query the receipt instead of causing a second publication.

The development loop is create -> validate -> play -> inspect -> compare -> revise. Its stop gates
include failed physics/replication checks, visual regressions, exhausted resource budgets and
unavailable evidence. An agent's assessment is not a substitute for the independent test oracle.
Keep screenshots of the engine distinct from generated concepts and profiler runs distinct from
uninstrumented performance baselines.

## Existing foundations and remaining gates

[`tools/tooling_smoke.py`](../tools/tooling_smoke.py) currently selects reviewed RenderDoc, Blender
and Tracy checks, with finite process lifetimes, output limits and explicit result artifacts.
[`tooling.md`](tooling.md) records their precise scope and isolation limitations. In particular,
RenderDoc frame replay is not a replay of an entire gameplay session, the Blender check is not a
game asset importer, and the Tracy standalone check is not Rust game instrumentation.

First wrap one existing native inspection/capture path, preserving its limits, and prove discovery,
invocation, error reporting, cancellation, missing-evidence rejection and artifact readback through
the actual agent connection. Next add session/input replay and comparison, then expose the
[map generator](map-generation.md) and [scenario authoring](gameplay-authoring.md) as they become
implemented. Test invalid inputs, unknown operations, stale/idempotent publication, tool crashes,
detached subprocess cleanup, cross-session access and real native output. Do not advertise an
operation merely because the adapter can return a placeholder success.

The first adapter profile serializes one owned inspection/capture job and inherits the current
launcher's 60-second phase deadline, 2 GiB/2,000-entry retained-evidence guard and existing per-tool
file limits from `tooling.md`; it cannot raise them through call arguments. Bound each request to
16 KiB and each structured inline response to 32 KiB, exposing larger checked artifacts separately.
New compile/generation/replay profiles need explicit numeric limits and failure tests before
advertisement; capture limits are not a universal policy for every future operation.
