# Haily personal assistant on Cellos

## Owner decision and status

The owner selected Haily as the planned first flagship application for Cellos.
Haily is a general-purpose personal assistant for coding, scientific research,
writing and other user-directed work; conversational input is an interaction
surface, not a chatbot-only product definition. These are product goals, not a
claim that every capability is implemented or qualified today.

The existing Rust application is developed at `/mnt/d/haily/` and runs on
Windows. Cellos porting/integration starts only after the owner considers Haily's
first version complete. No release date, version tag or completion criteria are
invented here; the entry review must bind the actual Haily release and its
accepted behavior. Finishing that version triggers review, not automatic
implementation or deployment admission.

**Status: recorded future application; implementation deferred.** In the meantime,
Cellos development continues on the Intel C2C substrate under
[ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md),
[current focus](current-focus.md) and the
[controlling contract plan](../../.agents/261011-1200-cell-contract-authority/plan.md).
This decision does not open an independent AI, GUI, browser, GPU/NPU or platform
program, release a legacy lock, approve an ABI, or activate remote/production
routes. Existing scoped authorizations retain their exact meaning.

## Application destination

- Preserve the assistant's broad scope: coding, scientific research, writing and
  other workflows selected by the user. A bounded demonstration validates one
  slice; it must not redefine the application as only chat or document review.
- Support native Cell execution and access through an HTTP ingress adapter using
  the same application identity, approval, task and result semantics.
- Use local tiny/small models as appropriate for the task and measured quality;
  no model prediction grants tool authority or proves a task is complete.
- Distribute agent work and model requests across identified Cellos PC/server
  nodes through explicit C2C contracts. One Cell remains resident on one node;
  distribution does not imply shared pointers, automatic migration or invisible
  failures.
- Preserve local-first privacy and explicit model/network egress policy. A local
  provider failure must not silently permit cloud egress.

The initial architectural recommendation is task/request distribution with
whole-model services resident on their nodes. Splitting one model's tensors or
layers across machines is a separate measured design decision, not an entry
requirement or a promised speedup. Runtime boundaries must be reviewed against
Haily's completed first version before this recommendation becomes a design.

## Why Haily is the flagship C2C consumer

Assessment: Haily is sufficiently broad to demonstrate Cellos's application-level
strengths without adding another showcase application. Interactive requests,
long-running research/coding/writing workflows, inference, indexing, tools,
approvals and persistent task state naturally exercise concurrency, authority,
resource control, lifecycle and distributed failure. Application evidence does
not replace kernel, DMA, isolation or hardware qualification.

The product destination is **useful on one supported PC; add admitted nodes to
increase service capacity or introduce capabilities without changing the user's
workflow**. Single-PC operation must support the accepted application scope
without requiring another compute node. External-service dependencies and egress
remain explicit; this is not a promise that every task works fully offline.

| Added-node benefit | What to demonstrate | What is not implied |
|---|---|---|
| Concurrent service capacity | More independent tasks/model requests, less contention with interactive work | Every request becomes faster or scaling is linear |
| Parallel workflow execution | Independent research, analysis and verification branches run concurrently | Sequential dependencies disappear |
| Additional capabilities | A node contributes an admitted model, dataset or tool/runtime suitable for new work | RAM/GPU becomes one shared memory pool or unsupported models become compatible |

Replicating tiny models increases serving capacity, not each model's reasoning
quality. Quality improvements require a better-suited model/tool or a workflow
whose coordination and verification have been evaluated. Whole-model serving and
task/request placement are the initial recommendation; tensor/layer splitting
requires separate evidence that its benefit exceeds network and synchronization
costs.

## Adding and removing nodes

The intended plug-in experience is discovery, user-authorized enrollment, exact
identity/version/resource/capability validation, preparation of required
artifacts, Ready admission, then bounded routing of suitable work. Discovery
alone must not authorize access to personal data or tools. Artifact presence,
advertised RAM and an authenticated node identity alone do not prove service
readiness or operation authority. Only compatible, qualified hardware is a
deployment target; this is not arbitrary-PC plug-and-play support.

Initially route new work to newly admitted nodes and retain affinity for active
stateful sessions/KV caches. Live task or KV migration is not an entry promise.
Planned removal drains work before retirement; unexpected loss preserves explicit
failed versus uncertain outcomes and application-specific reconciliation. Adding
nodes does not automatically make the coordinator or committed state highly
available.

## Application-level C2C acceptance scenario

Use a mixed personal-assistant session: interactive writing continues while
scientific-research/document analysis and an approved coding task run in the
background. Establish the single-PC baseline, enroll a second compatible node,
observe actual distribution of new work, then exercise worker failure, node
disconnect and planned drain. Preserve quality, approval and provenance checks
through every transition; a throughput improvement that weakens them is not a
pass.

Measure interactive latency under background load, task success/output quality,
independent-work throughput, CPU/RAM/network/storage, coordinator overhead,
recovery time and duplicate/lost/unauthorized effects. Compare equivalent Haily
execution on Linux with the same models, hardware and semantics. Distributed
agents also run on Linux; Cellos must demonstrate a concrete operational,
control or cost advantage rather than infer superiority from a Cell/node count.
Acceptance thresholds are set from the pinned workloads at entry, not invented
in this roadmap. These are future acceptance requirements, not measured results.

## Cellos readiness work while Haily matures

Each implementation slice still needs its current contract/ownership admission;
this table identifies application needs, not a parallel work queue or proof of
readiness. Reuse retained evidence at its original ceiling.

| Application need | Cellos readiness condition | Existing owner |
|---|---|---|
| Safe tools and delegation | Typed attenuated/revocable authority, authenticated callers, scoped resource access and explicit approval boundaries | Controlling P02/P05; application policy remains outside the kernel |
| Useful model/agent capacity | Measured CPU/thread/SMP budgets and memory/VA, stack, weights, scratch and per-session KV high-water for exact workloads | P03/P10; inference/runtime additions need a named admitted consumer slice |
| Concurrent tasks and streams | Bounded operations, waits, cancellation, terminal capacity and backpressure; slow inference does not block unrelated work indefinitely | P04 and reviewed runtime/service adapters |
| Task state and recovery | Committed business state distinct from execution; recovery and side-effect reconciliation with explicitly evidenced persistence guarantees | P06; G2-R-core/B alone does not prove crash-preserving acknowledgement, and G2-J remains separately gated |
| Haily host dependencies | Exact contracts for async runtime, storage, credentials, networking and any process/FFI/browser tools actually required by the selected workflows | P07; no blanket Tokio, SQLite, POSIX or desktop compatibility claim |
| Cross-node work | Authenticated/protected peer authority, explicit uncertain outcomes, bounded routing/placement/application budgets and safe drain | P08; no implicit remote fallback or automatic retry of uncertain side effects |
| Legacy tool participants | Qualified isolated native or guest execution and explicit service adapters only where required | P07/P09; Intel VMX/guest participation has its own gates |
| Physical deployment | Qualified first Intel node, then a separately qualified second node; exact NIC/storage/firmware/DMA and operational captures | P05/P10 and the [HCL](../hardware-compatibility-list.md) |

## Deferred rollout roadmap

The stages below are ordered acceptance gates, not dated commitments or current
implementation permissions. Review/qualification gates apply to each stage.

| Stage | Work after Haily first-version completion | Exit evidence |
|---|---|---|
| H0 — Release and portability review | Pin Haily release/source identity, supported workflows, dependencies, tool authority, model artifacts/licenses and local/cloud policy. Reuse existing Cellos AI/Hypha/HTTP pieces where compatible; decide the exact port and retained external-worker boundaries. | Owner-accepted behavior matrix and scoped migration plan; unsupported capabilities explicitly identified, not silently removed |
| H1 — Single-node application | Port the selected assistant core and required adapters; make the accepted scope useful on one supported PC without another compute node; exercise native and HTTP entry into the same task/approval/state path. | Real selected coding, scientific-research and writing scenarios covering the accepted first-version scope; correct approvals, cancellation and denied access; mixed-session latency/resource/quality baseline and explicit external-service dependencies |
| H2 — Distributed application | Enroll a second compatible node through identity/capability/Ready admission; place agent workers and resident model services with bounded task/result streaming, affinity, routing and failure handling. | Actual new-work distribution and measured capacity/capability benefit; node disconnect, worker restart, stale responses, queue pressure, planned drain and uncertain-outcome reconciliation; unchanged quality/authority checks; software evidence labelled separately from physical captures |
| H3 — Physical Intel qualification and measurement | Run accepted workflows on two identified qualified Intel machines; compare equivalent Linux execution using the same models, hardware and semantics. | Quality/task-success, latency distributions, throughput, CPU/RAM/network/storage, recovery and side-effect evidence; physical captures, not QEMU extrapolation |
| H4 — Scoped deployment | Admit the agreed workflow set and operating environment with provisioned identities/credentials, persistent recovery, update/rollback and operational procedures. | Applicable independent/security/production approvals and release evidence; no claim of unrestricted "all tasks" support from a narrow demo |

A first rollout may use a bounded workflow set, but reducing the agreed Haily
application scope requires an explicit owner decision. A scientific-research
workflow must preserve source/provenance and verification requirements; fluent
text alone is not evidence of scientific correctness. Coding and other mutating
tools must retain approval, isolation and result verification rather than treating
model confidence as authorization.

## Entry and evidence rules

- Haily first-version completion is necessary for starting the port; it does not
  imply that Cellos, every tool runtime or every model is ready.
- Review exact architecture, tokenizer, quantization, resident memory, context,
  concurrency, license and quality before selecting a local model. GGUF format
  alone does not establish backend compatibility.
- HTTP is an adapter, not a substitute for application authority or a bypass of
  C2C admission. Native and HTTP clients must observe equivalent task outcomes.
- Do not share a SQLite file across nodes as a cluster control plane. Pin the
  state owner and define reconciliation before adding failover or replication.
- Retain explicit pre-dispatch refusal versus post-dispatch uncertainty; C2C
  transport does not guarantee exactly-once business effects.
- No universal performance superiority, distributed-model speedup, physical
  qualification or production readiness follows from this roadmap.
