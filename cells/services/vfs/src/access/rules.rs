//! The policy data behind [`crate::access::AccessTable`].
//!
//! Two tables, consulted in this order:
//!
//! 1. [`EXACT_RULES`] — decisions for one whole path.
//! 2. [`PREFIX_RULES`] — first matching prefix wins; the fallback.
//!
//! Rules are keyed by **filesystem path**, never by `CellId`. A `CellId` is
//! `CellId(tid)`, so a service that init respawns after a crash comes back under
//! a different one; a table keyed that way could not be written down ahead of
//! time and would point at the wrong cell after the first restart.
//!
//! Nothing here is keyed by *cell* either. Scoping a rule to "only the net cell"
//! needs a binding from the calling cell to which program it is running, and the
//! cell name is no such binding: it is the last component of the `path_hint` its
//! spawner passed, which the spawner chooses freely. A cell spawned as
//! `path_hint = "/bin/vfs"` would inherit whatever the rule granted `/bin/vfs`.
//! The one program-scoped rule, `/srv/sshd`, matches the kernel-measured image
//! instead: a signed image declaring that program name (`access::srv_owner`).

/// Read/write decision for a path or a prefix.
pub struct PathRule {
    /// The path this rule applies to: a full path in [`EXACT_RULES`], a prefix in
    /// [`PREFIX_RULES`].
    pub prefix: &'static str,
    /// True if any cell may read here.
    pub allow_read_all: bool,
    /// True if any cell may write here.
    pub allow_write_all: bool,
}

/// Whole-path rules, checked before any prefix rule.
///
/// Empty: every path that needs a narrower rule than its prefix gives also needs
/// to name *which* cell it is narrower for, and see the module note on why that
/// binding does not exist yet. The lookup ships now so adding a row later is a
/// data change rather than a control-flow change.
pub static EXACT_RULES: &[PathRule] = &[
    PathRule {
        prefix: "/srv/cellos",
        allow_read_all: false,
        allow_write_all: false,
    },
    // Phase 09's SSH host-trust store directory, retired by Phase 12 (kms now
    // keeps the store). A disk provisioned before it may still hold the old
    // store; nobody lists, reads, writes or replaces anything there.
    PathRule {
        prefix: "/srv/.cellos-trust",
        allow_read_all: false,
        allow_write_all: false,
    },
    // ADR-0029 §3: the machine fingerprint pin. Only the live `MACHINE_PIN`
    // provider (trusted init) is admitted (`access::machine_pin`).
    PathRule {
        prefix: "/srv/.cellos-machine",
        allow_read_all: false,
        allow_write_all: false,
    },
    // The sshd host-key seed and authorized_keys. Only a signed image declaring
    // the program name `sshd` is admitted (`access::srv_owner`).
    PathRule {
        prefix: "/srv/sshd",
        allow_read_all: false,
        allow_write_all: false,
    },
    PathRule {
        prefix: crate::access::GUEST_DISK_PATH,
        allow_read_all: true,
        allow_write_all: false,
    },
];

/// Prefix rules, first match wins. Ordered specific → general; `/` is last and
/// matches every absolute path, so a path that reaches it is decided by it.
///
/// Deliberately broad on reads. Narrowing reads and boot-time reads are the same
/// set right now (the loader reads every cell ELF through `/bin/`, the shell
/// lists `/srv/`), so a narrow table would fail closed on the boot path and the
/// repair would be to reopen all of it. Reads narrow per prefix, once each
/// prefix's real readers are known.
pub static PREFIX_RULES: &[PathRule] = &[
    // The loader reads cell ELFs through here on every spawn.
    PathRule {
        prefix: "/bin/",
        allow_read_all: true,
        allow_write_all: false,
    },
    PathRule {
        prefix: "/data/",
        allow_read_all: true,
        allow_write_all: true,
    },
    PathRule {
        prefix: "/tmp/",
        allow_read_all: true,
        allow_write_all: true,
    },
    // FAT32 interop volume.
    PathRule {
        prefix: "/mnt/sd/",
        allow_read_all: true,
        allow_write_all: true,
    },
    // Secondary disks' FAT32 volumes (`/mnt/disk/<name>`): the /mnt/sd policy.
    PathRule {
        prefix: "/mnt/disk/",
        allow_read_all: true,
        allow_write_all: true,
    },
    // /srv service volume (CellosFS Native on P5). Writable like /data: without
    // this rule every /srv write fell through to the read-only "/" rule and
    // failed before reaching the backend.
    PathRule {
        prefix: "/srv/cellos/",
        allow_read_all: false,
        allow_write_all: false,
    },
    PathRule {
        prefix: "/srv/.cellos-trust/",
        allow_read_all: false,
        allow_write_all: false,
    },
    PathRule {
        prefix: "/srv/.cellos-machine/",
        allow_read_all: false,
        allow_write_all: false,
    },
    PathRule {
        prefix: "/srv/sshd/",
        allow_read_all: false,
        allow_write_all: false,
    },
    PathRule {
        prefix: "/srv/",
        allow_read_all: true,
        allow_write_all: true,
    },
    // Root: readable, read-only. Reads stay open because the ramfs root holds
    // paths outside every prefix above that cells read at startup (the cluster
    // config at /etc/cellos/cluster.cfg is one), and denying them here would
    // break those cells with an opaque PermissionDenied.
    PathRule {
        prefix: "/",
        allow_read_all: true,
        allow_write_all: false,
    },
];
