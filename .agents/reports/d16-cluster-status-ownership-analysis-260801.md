# D16 — Cluster status ownership and the false binary status

**Status:** ruled/applied 2026-08-01. Docs updated; no code or ABI changed.

**Question:** which document owns cluster status, and should `system-architecture.md`
copy Spec 20's split table verbatim?

## Answer first

**Spec 20 should own the proposed cluster/IPC contract, but neither Spec 20 nor
`system-architecture.md` should own hand-written implementation status.** Spec 21 assigns
status to the generated Layer-3 status file. `system-architecture.md` should retain one
stable architecture summary and link to Spec 20 plus generated status; it should not copy
the split table verbatim.

The current choice is not “all planned” versus “transport fully built.” The repository has
four distinct states:

1. **Broker boot + NodeId generation are wired.** `net-broker` starts, obtains entropy,
   generates an X25519 static key, derives/logs a NodeId, and enters its dispatch loop.
2. **Noise, relay, connection, discovery, gossip, lease, and enrollment modules contain
   substantial source code and compile, but most are not wired into the loop.** Code
   presence is not runtime delivery.
3. **Typed remote forwarding is a stub.** `main::dispatch` contains only TODOs.
4. **Remote call/watch has no working runtime.** Spec 20 correctly says this contract is
   draft, but its “transport / NodeId / relay built” row still over-compresses the first two
   states.

## 1. What executes today

`cell_main` creates `BrokerRng`, a `StaticKeypair`, and `BrokerIdentity`, then logs the
NodeId (`cells/services/net-broker/src/main.rs:96-115`). It constructs a `RelayClient`, but
the loop only calls `relay_client.is_connected()`; no connect, receive, send, Noise
session, beacon, lease, gossip, or enrollment work is driven (`:117-149`).

The sole dispatch function is empty apart from P06/P08/P09 TODOs (`:152-157`). Therefore
an ostd `ClusterRef::lookup_remote` request can reach the broker, but the broker never
replies. The client then waits in `sys_recv` (`libs/ostd/src/cluster.rs:54-92`). The public
API is present; the advertised operation is not usable.

`routing.rs` contains `RemoteServiceProxy` and a lookup wire format, but the module itself
states it is not wired. Even its success response is only the broker's own TID with a
future “forward via Noise” comment (`cells/services/net-broker/src/routing.rs:139-172`).

The transport and relay modules are real implementations at the type/function level:

- `NoiseSession` binds cluster and local/remote NodeIds into the prologue and implements
  handshake/record operations (`transport.rs:109-176` and following).
- `RelayClient` implements connect/register/send/receive framing (`relay.rs:52-199`).
- `ConnectionManager` attempts direct sessions, but existing-session lookup always returns
  `None` and relay-mediated Noise ends in a TODO/`NotFound`
  (`connection_manager.rs:53-102`).

These modules are largely `allow(dead_code)` scaffolding. `cargo check -p
service-net-broker` passes, which proves they compile—not that two nodes communicate.

## 2. Test evidence

The only cluster integration file is `tests/integration/tests/cluster-boot.rs`. Its active
test checks one-node broker boot, entropy, and key generation. Service lookup is ignored
until a shell command exists (`:124-149`). There is no two-node handshake, relay,
RemoteServiceProxy, remote request/reply, partition, watch, gossip, or enrollment runtime
test.

Accordingly:

- `system-architecture.md:868` saying **all** cluster work is planned is false;
- Spec 20's blanket “transport / NodeId / relay built” is too strong for operational
  status—only NodeId boot is wired, while transport/relay implementations are dormant;
- roadmap/changelog statements that all ten phases shipped, remote calls work, a two-node
  testbed ran, or the swarm is production-ready are also unsupported by the current call
  graph and tests.

## 3. Ownership under Spec 21

Spec 21's allocation rule is decisive:

- Layer 1 specs own stable decisions, rationale, invariants, rejected alternatives, and
  deliberate absences.
- Layer 3 generated status owns what exists/works today.
- architecture summaries link rather than copy volatile status.

Spec 20 is Draft and can own the proposed `CellAddr`, identity, failure, and remote-watch
contract. Its hand-written foundation status table is useful evidence today but violates
the long-term ownership rule. Copying it into system architecture would create a third
status source after roadmap/changelog and preserve the exact drift D16 exposed.

## Recommended ruling [FINAL]

**Approve recommendation A:**

1. Spec 20 owns the cluster/remote-IPC contract, not implementation status.
2. Generated Layer-3 status owns the split implementation state. Add anchors/checks that
   can distinguish at least: code present, wired from `cell_main`, and runtime-tested.
3. `system-architecture.md` replaces “all planned” with a stable summary and links to
   Spec 20/generated status; it does **not** reproduce the table verbatim.
4. Until generation exists, correct all hand-written status claims to the four-state split
   above and label them transitional, not authoritative.
5. Correct roadmap/changelog false-completion claims in the same pass.
6. Do not call the remote foundation shipped until a two-node gate proves Noise
   handshake, lookup, request/reply forwarding, timeout/partition behaviour, and broker
   restart handling.

### Rejected alternatives

- **System architecture owns status:** duplicates volatile detail and conflicts with
  Spec 21.
- **Copy Spec 20's table verbatim:** duplicates an already over-compressed table.
- **Treat module existence as shipped transport:** no dispatch-loop caller or two-node
  runtime evidence exists.
- **Call everything planned:** erases the broker/NodeId boot work and substantial compiled
  modules.
