#!/usr/bin/env python3
"""Host tests for `scripts/assemble-x86-image.py` (no cargo, no signing key).

Builder, signer, policy and provenance are fake hooks; the image writer
(`tools/mkfat32.py`, with template-generated boot config) and the read-back
(`scripts/generate-boot-config.py` `image_files`) are the real ones.

    python3 -B scripts/test_assemble_x86_image.py
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

REPO = Path(__file__).resolve().parent.parent


def load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


asm = load("assemble_x86_image", REPO / "scripts" / "assemble-x86-image.py")
mkfat = load("assemble_test_mkfat32", REPO / "tools" / "mkfat32.py")
reader = load("assemble_test_reader", REPO / "scripts" / "generate-boot-config.py")
signing = asm.signing

HEADLESS, DESKTOP = asm.PROFILES["headless"], asm.PROFILES["desktop"]
CONFIG = sorted(asm.CONFIG_PATHS)


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def snapshot(directory: Path) -> dict[str, bytes]:
    """Every file under `directory` (relative path -> bytes), directories included."""
    if not directory.exists():
        return {}
    return {str(p.relative_to(directory)): (p.read_bytes() if p.is_file() else b"<dir>")
            for p in sorted(directory.rglob("*"))}


class ManifestTables(unittest.TestCase):
    """The profiles are exactly the S10-1.2 tables."""

    def test_headless_packages_and_paths(self):
        self.assertEqual(HEADLESS.packages(), [
            "app-cellbox", "service-vfs", "service-config", "service-input", "service-net",
            "service-platform", "driver-nvme", "driver-e1000", "driver-igb", "driver-ahci",
            "driver-xhci", "driver-serial-x86", "app-sys-tools", "tier2-smoke", "tier2-exploit"])
        self.assertEqual(HEADLESS.image_paths(), {"/bin/" + name for name in (
            "cellbox", "cellbox-ssh", "vfs", "config", "net", "input", "platform", "nvme",
            "e1000", "igb", "ahci", "xhci", "serial", "ls", "cat", "echo", "ps", "kill",
            "shutdown", "tier2-smoke", "tier2-exploit")} | {"/POLICY.BIN"})
        self.assertEqual(HEADLESS.init_features, ("input",))

    def test_desktop_is_headless_plus_the_desktop_apps(self):
        self.assertEqual(DESKTOP.packages(), HEADLESS.packages() + [
            "service-compositor", "fb-console", "desktop", "files", "terminal", "app-chrome", "ocel"])
        self.assertEqual(DESKTOP.image_paths() - HEADLESS.image_paths(), {"/bin/" + name for name in (
            "compositor", "fb-console", "desktop", "files", "terminal", "chrome", "ocel")})
        self.assertTrue(HEADLESS.image_paths() < DESKTOP.image_paths())
        self.assertEqual(DESKTOP.init_features, ("input", "ui"))

    def test_alias_shares_one_source_and_retired_paths_are_absent(self):
        for profile in (HEADLESS, DESKTOP):
            owners = [cell for cell in profile.cells
                      if {"/bin/cellbox", "/bin/cellbox-ssh"} & set(cell.paths)]
            self.assertEqual(len(owners), 1)
            self.assertEqual(owners[0].paths, ("/bin/cellbox", "/bin/cellbox-ssh"))
            self.assertEqual(owners[0].binary, "cellbox")
            self.assertFalse(profile.image_paths() & {"/bin/shell", "/bin/sshd", "/bin/init"})

    def test_worker_set_is_d10_6(self):
        workers = {cell.binary for cell in HEADLESS.cells if cell.package == "app-sys-tools"}
        self.assertEqual(workers, {"ls", "cat", "echo", "ps", "kill", "shutdown"})

    def test_only_tier2_fixtures_are_unsigned(self):
        for profile in (HEADLESS, DESKTOP):
            self.assertEqual({cell.binary for cell in profile.cells if not cell.signed},
                             {"tier2-smoke", "tier2-exploit"})

    def test_blocked_set_is_the_signers_own(self):
        self.assertIs(asm.signing, sys.modules["cellos_sign.signing"])
        self.assertTrue(signing.BLOCKED_ARTIFACT_SHA256 <= asm.Assembler(
            HEADLESS, Path("/nonexistent")).blocked)


class ImageFixture:
    """A real image written by mkfat32 from per-path fixture bytes."""

    def __init__(self, root: Path, profile, *, drop=(), add=(), alias_differs=False,
                 features=None, template_edit=None):
        self.profile = profile
        inputs = root / "inputs"
        inputs.mkdir()
        pairs = []
        for cell in profile.cells:
            source = inputs / cell.binary
            source.write_bytes(b"\x7fELF fixture " + cell.binary.encode())
            for path in cell.paths:
                if path == "/bin/cellbox-ssh" and alias_differs:
                    other = inputs / "cellbox-other"
                    other.write_bytes(b"\x7fELF different alias bytes")
                    pairs.append((other, path))
                else:
                    pairs.append((source, path))
        policy = inputs / "POLICY.BIN"
        policy.write_bytes(b"signed policy fixture")
        pairs.append((policy, "/POLICY.BIN"))
        for path, data in add:
            extra = inputs / ("extra" + path.replace("/", "_"))
            extra.write_bytes(data)
            pairs.append((extra, path))
        pairs = [(src, dst) for src, dst in pairs if dst not in drop]
        template_dir = None
        if template_edit is not None:
            template_dir = root / "templates"
            shutil.copytree(REPO / "config", template_dir)
            template_edit(template_dir)
        self.config_dir = root / "boot-config"
        image = root / "kernel_fs.img"
        features = ",".join(profile.init_features) if features is None else features
        mkfat.create_fat32_image(str(image), [(str(s), d) for s, d in pairs], features,
                                 config_dir=template_dir, config_output_dir=self.config_dir)
        self.files = reader.image_files(image)
        self.expected = {dst: src.read_bytes() for src, dst in pairs}
        for path in CONFIG:
            self.expected[path] = (self.config_dir / Path(path).name).read_bytes()
        # check_image compares against the manifest; dropped paths stay "expected".
        for cell in profile.cells:
            for path in cell.paths:
                self.expected.setdefault(path, b"\x7fELF fixture " + cell.binary.encode())

    def check(self, blocked=frozenset()):
        expected = {path: data for path, data in self.expected.items()
                    if path in self.profile.image_paths() | asm.CONFIG_PATHS}
        asm.check_image(self.files, expected, self.profile, frozenset(blocked))


class OutputChecks(unittest.TestCase):
    def fixture(self, profile=HEADLESS, **kwargs) -> ImageFixture:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        return ImageFixture(Path(tmp.name), profile, **kwargs)

    def test_clean_images_pass_and_boot_cellbox_sh_from_templates(self):
        for profile in (HEADLESS, DESKTOP):
            with self.subTest(profile=profile.name):
                image = self.fixture(profile)
                image.check()
                self.assertEqual(set(image.files), profile.image_paths() | asm.CONFIG_PATHS)
                autoload = image.files["/etc/cellos/autoload.toml"].decode()
                self.assertIn('path = "/bin/cellbox"\nargs = ["sh"]', autoload)

    def test_refuses_an_extra_bin_shell(self):
        image = self.fixture(add=[("/bin/shell", b"\x7fELF split shell")])
        with self.assertRaisesRegex(asm.AssemblyError, "/bin/shell"):
            image.check()

    def test_refuses_an_extra_bin_sshd(self):
        image = self.fixture(add=[("/bin/sshd", b"\x7fELF inbound service")])
        with self.assertRaisesRegex(asm.AssemblyError, "/bin/sshd"):
            image.check()

    def test_refuses_any_other_extra_file(self):
        image = self.fixture(add=[("/welcome.md", b"residue")])
        with self.assertRaisesRegex(asm.AssemblyError, r"extra \['/welcome.md'\]"):
            image.check()

    def test_refuses_a_missing_alias(self):
        image = self.fixture(drop={"/bin/cellbox-ssh"})
        with self.assertRaisesRegex(asm.AssemblyError, r"missing \['/bin/cellbox-ssh'\]"):
            image.check()

    def test_refuses_alias_bytes_that_differ(self):
        image = self.fixture(alias_differs=True)
        with self.assertRaisesRegex(asm.AssemblyError, "not byte-equal"):
            image.check()

    def test_refuses_a_file_that_differs_from_its_input(self):
        image = self.fixture()
        image.expected["/bin/vfs"] = b"other bytes"
        with self.assertRaisesRegex(asm.AssemblyError, "differs from its input: /bin/vfs"):
            image.check()

    def test_refuses_a_blocked_hash_in_any_file(self):
        image = self.fixture()
        for path, data in sorted(image.files.items()):
            # The alias shares cellbox's bytes; the first path holding them is named.
            first = min(p for p, d in image.files.items() if d == data)
            with self.subTest(path=path):
                with self.assertRaisesRegex(asm.AssemblyError, "quarantined bytes in the image: "
                                            + first + "$"):
                    image.check(blocked={sha(data)})

    def test_refuses_an_autoload_row_for_bin_shell(self):
        def edit(templates: Path):
            with (templates / "autoload.toml").open("a") as handle:
                handle.write('\n[[cells]]\nname = "shell"\npath = "/bin/shell"\nrestart = "always"\n')
        image = self.fixture(template_edit=edit)
        with self.assertRaisesRegex(asm.AssemblyError, "autoload starts a refused path: /bin/shell"):
            image.check()

    def test_refuses_a_second_cellbox_row_or_one_without_sh(self):
        def twice(templates: Path):
            with (templates / "autoload.toml").open("a") as handle:
                handle.write('\n[[cells]]\nname = "cellbox2"\npath = "/bin/cellbox"\nargs = ["sh"]\n')
        def no_args(templates: Path):
            text = (templates / "autoload.toml").read_text().replace('args = ["sh"]\n', "")
            (templates / "autoload.toml").write_text(text)
        for edit in (twice, no_args):
            with self.subTest(edit=edit.__name__):
                with self.assertRaisesRegex(asm.AssemblyError, "exactly one /bin/cellbox"):
                    self.fixture(template_edit=edit).check()

    def test_refuses_a_services_row_for_sshd_or_config(self):
        rows = {
            "sshd": '\n[[cells]]\nname = "sshd"\npath = "/bin/sshd"\nafter = ["net"]\nrestart = "always"\n',
            "config": "",  # the template row is selected once the ai feature is on
        }
        for name, row in rows.items():
            with self.subTest(service=name):
                def edit(templates: Path, row=row):
                    with (templates / "services.toml").open("a") as handle:
                        handle.write(row)
                features = "input,ai" if name == "config" else None
                image = self.fixture(template_edit=edit if row else None, features=features)
                with self.assertRaisesRegex(asm.AssemblyError,
                                            "services.toml starts a refused service: " + name):
                    image.check()

    def test_refuses_a_desktop_image_without_compositor(self):
        # Without the binary the template writer already refuses (desktop/fb-console
        # run after compositor), so no such image can be written.
        with self.assertRaisesRegex(ValueError, "unavailable dependency compositor"):
            self.fixture(DESKTOP, drop={"/bin/compositor"})
        # The binary present but not started (config generated without `ui`).
        unstarted = self.fixture(DESKTOP, features="input")
        with self.assertRaisesRegex(asm.AssemblyError, "does not start compositor"):
            unstarted.check()


class FakeAssembler(asm.Assembler):
    """Real input/image/output checks and publish; fake cargo, signer, policy."""

    def __init__(self, *args, fail_build=False, omit=(), old=(), unreported=(),
                 refuse_signing=False, **kwargs):
        super().__init__(*args, **kwargs)
        self.fail_build, self.omit, self.old = fail_build, set(omit), set(old)
        self.unreported, self.refuse_signing = set(unreported), refuse_signing
        self.present_at_build: list[str] = []
        self.signed: list[str] = []
        self.signed_paths: list[Path] = []
        self.candidate_roots: list[Path] = []
        self.cargo_calls = 0

    def cargo_build(self, args):
        self.cargo_calls += 1
        names = ([asm.INIT_BINARY] if args[:2] == ["-p", asm.INIT_PACKAGE] else
                 [cell.binary for cell in self.profile.cells])
        self.present_at_build += [name for name in names if self.binary(name).exists()]
        if self.fail_build:
            raise asm.AssemblyError("cargo build failed (exit 101): fake")
        self.rel.mkdir(parents=True, exist_ok=True)
        reported = set()
        for name in dict.fromkeys(names):
            if name in self.omit:
                continue
            self.binary(name).write_bytes(b"\x7fELF built " + name.encode())
            if name not in self.unreported:
                reported.add(self.binary(name).resolve())
        return reported

    def placed_ns(self, path):
        if path.name in self.old and path.parent == self.rel:
            return 0
        return super().placed_ns(path)

    def sign(self, targets, candidate_root):
        if self.refuse_signing:
            raise signing.SigningRefused("ROLLOUT BLOCKED: unqualified Cellbox/terminal artifact")
        self.candidate_roots.append(candidate_root)
        for target in targets:
            target.write_bytes(target.read_bytes() + b" +signed")
            self.signed.append(target.name)
            self.signed_paths.append(target)

    def sign_policy(self, out):
        out.write_bytes(b"fake signed policy")

    def provenance(self):
        return {"git_head": "0" * 40, "dirty_digest_sha256": "0" * 64}


class EndToEnd(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        env = mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": str(self.root / "target")})
        env.start()
        self.addCleanup(env.stop)
        self.out = self.root / "out"
        self.out.mkdir()
        (self.out / "kernel_fs.img").write_bytes(b"previous image")
        (self.out / "init").write_bytes(b"previous init")
        (self.out / "boot-config").mkdir()
        (self.out / "boot-config" / "autoload.toml").write_bytes(b"previous autoload")
        (self.out / "manifest.json").write_bytes(b"{}")
        self.before = snapshot(self.out)

    def assembler(self, profile=HEADLESS, **kwargs) -> FakeAssembler:
        return FakeAssembler(profile, self.out, **kwargs)

    def assert_refused(self, assembler, pattern, error=asm.AssemblyError):
        with self.assertRaisesRegex(error, pattern):
            assembler.assemble()
        self.assertEqual(snapshot(self.out), self.before)

    def test_success_publishes_image_init_config_and_manifest_together(self):
        for profile in (HEADLESS, DESKTOP):
            with self.subTest(profile=profile.name):
                manifest = self.assembler(profile, seed_hex="11" * 32).assemble()
                self.assertEqual(sorted(p.name for p in self.out.iterdir()),
                                 ["boot-config", "init", "kernel_fs.img", "manifest.json"])
                self.assertEqual(sorted(p.name for p in (self.out / "boot-config").iterdir()),
                                 ["autoload.toml", "services.toml", "system.toml"])
                on_disk = json.loads((self.out / "manifest.json").read_text())
                self.assertEqual(on_disk, manifest)
                image = (self.out / "kernel_fs.img").read_bytes()
                self.assertEqual(on_disk["kernel_fs_img"], {"sha256": sha(image), "size": len(image)})
                init = (self.out / "init").read_bytes()
                self.assertEqual(on_disk["init"]["sha256"], sha(init))
                self.assertEqual(init, b"\x7fELF built app-init +signed")
                files = reader.image_files(self.out / "kernel_fs.img")
                self.assertEqual(set(on_disk["files"]), set(files))
                for path, data in files.items():
                    self.assertEqual(on_disk["files"][path]["sha256"], sha(data), path)
                for path in CONFIG:
                    self.assertEqual(files[path],
                                     (self.out / "boot-config" / Path(path).name).read_bytes())
                self.assertEqual(files["/bin/cellbox"], files["/bin/cellbox-ssh"])
                self.assertEqual(files["/bin/tier2-smoke"], b"\x7fELF built tier2-smoke")
                self.assertEqual(on_disk["key_class"], "private")
                self.assertEqual(on_disk["profile"], profile.name)

    def test_dev_key_class_without_a_seed(self):
        self.assertEqual(self.assembler().assemble()["key_class"], "dev")

    def test_manifest_records_the_canonical_fleet_kernel_features(self):
        self.assertEqual(asm.CANONICAL_KERNEL_FEATURES, ("policy-required",))
        for profile in (HEADLESS, DESKTOP):
            with self.subTest(profile=profile.name):
                self.assembler(profile).assemble()
                manifest = json.loads((self.out / "manifest.json").read_text())
                self.assertEqual(manifest["kernel_features"], ["policy-required"])

    def test_invalidation_removes_stale_inputs_before_the_build(self):
        assembler = self.assembler(fail_build=True)
        assembler.rel.mkdir(parents=True)
        for name in ("cellbox", "service-net", "ls", "app-init", "tier2-smoke"):
            assembler.binary(name).write_bytes(b"STALE from an earlier build")
        unrelated = assembler.binary("hotswap")
        unrelated.write_bytes(b"not a manifest input")
        self.assert_refused(assembler, "cargo build failed")
        self.assertEqual(assembler.present_at_build, [])
        self.assertFalse(any(assembler.binary(n).exists() for n in ("cellbox", "app-init", "ls")))
        self.assertTrue(unrelated.exists())

    def test_builder_failure_leaves_the_out_dir_byte_identical(self):
        self.assert_refused(self.assembler(fail_build=True), "cargo build failed")

    def test_missing_artifact_is_refused(self):
        for name in ("service-net", "app-init", "kill"):
            with self.subTest(missing=name):
                self.assert_refused(self.assembler(omit={name}), "produced no artifact: .*" + name)

    def test_artifact_older_than_the_build_start_is_refused(self):
        self.assert_refused(self.assembler(old={"driver-xhci"}), "older than the build start")

    def test_artifact_not_reported_by_cargo_is_refused(self):
        self.assert_refused(self.assembler(unreported={"ps"}), "not reported by this cargo run")

    def test_signer_refusal_leaves_the_out_dir_untouched(self):
        self.assert_refused(self.assembler(refuse_signing=True), "ROLLOUT BLOCKED",
                            signing.SigningRefused)

    def test_blocked_input_hash_is_refused_before_the_image(self):
        blocked = frozenset({sha(b"\x7fELF built tier2-smoke")})
        assembler = self.assembler(extra_blocked=blocked)
        with mock.patch.object(FakeAssembler, "write_image") as writer:
            self.assert_refused(assembler, "quarantined bytes refused as image input")
            writer.assert_not_called()

    def test_signer_only_sees_signed_cells_and_init(self):
        assembler = self.assembler()
        assembler.assemble()
        self.assertEqual(sorted(assembler.signed), sorted(
            {cell.binary for cell in HEADLESS.cells if cell.signed} | {"app-init"}))

    def test_injected_retired_paths_are_refused(self):
        source = self.root / "legacy"
        source.write_bytes(b"\x7fELF legacy")
        for path in ("/bin/shell", "/bin/sshd"):
            with self.subTest(path=path):
                self.assert_refused(self.assembler(extra_pairs=((source, path),)),
                                    "retired or forbidden image paths refused")

    def test_missing_out_dir_is_created_only_on_success(self):
        self.out = self.root / "fresh"
        self.before = {}
        self.assert_refused(self.assembler(fail_build=True), "cargo build failed")
        self.assertFalse(self.out.exists())
        self.assembler().assemble()
        self.assertTrue((self.out / "kernel_fs.img").is_file())

    def test_dev_key_run_inside_the_repository_is_refused_before_invalidation(self):
        stale_dir = self.root / "target" / "x86_64-unknown-none" / "release"
        stale_dir.mkdir(parents=True)
        stale = stale_dir / "cellbox"
        stale.write_bytes(b"stale input")
        link = self.root / "into-repo"
        link.symlink_to(REPO / "kernel" / "src")
        for out_dir in (REPO / "kernel" / "src" / "embedded-x86_64" / "sub", REPO / "kernel" / "src",
                        REPO / "build" / "candidate", link / "other"):
            with self.subTest(out_dir=str(out_dir)):
                assembler = FakeAssembler(HEADLESS, out_dir)
                with self.assertRaisesRegex(asm.AssemblyError, "--out-dir .* inside the repository"):
                    assembler.assemble()
                self.assertEqual(assembler.cargo_calls, 0)
                self.assertEqual(stale.read_bytes(), b"stale input")
        self.assertFalse((REPO / "build" / "candidate").exists())
        for target_dir in (REPO / "target", "target"):
            with self.subTest(target_dir=str(target_dir)), \
                    mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": str(target_dir)}):
                assembler = FakeAssembler(HEADLESS, self.out)
                with self.assertRaisesRegex(asm.AssemblyError, "CARGO_TARGET_DIR .* inside the repository"):
                    assembler.assemble()
                self.assertEqual(assembler.cargo_calls, 0)
        self.assertEqual(snapshot(self.out), self.before)

    def test_canonical_route_admits_only_its_own_roots(self):
        canonical = REPO / "kernel" / "src" / "embedded-x86_64"
        outside = self.root / "outside-target"
        for profile in (HEADLESS, DESKTOP):
            for target_dir in (None, REPO / "target", outside):
                env = {} if target_dir is None else {"CARGO_TARGET_DIR": str(target_dir)}
                with self.subTest(profile=profile.name, target=str(target_dir)), \
                        mock.patch.dict(os.environ, env):
                    if target_dir is None:
                        os.environ.pop("CARGO_TARGET_DIR", None)
                    assembler = FakeAssembler(profile, canonical)
                    assembler.check_isolation()  # only the check: nothing is assembled
                    self.assertEqual(assembler.route_name(), "x86-canonical-" + profile.name)
            with self.subTest(profile=profile.name, candidate=True):
                self.assertEqual(FakeAssembler(profile, self.out).route_name(),
                                 "x86-candidate-" + profile.name)
        for target_dir in (REPO / "build" / "t", REPO / "target" / "sub", REPO / "kernel"):
            with self.subTest(target_dir=str(target_dir)), \
                    mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": str(target_dir)}):
                with self.assertRaisesRegex(asm.AssemblyError, "CARGO_TARGET_DIR .* inside the repository"):
                    FakeAssembler(HEADLESS, canonical).check_isolation()

    def test_private_seed_runs_keep_the_s10_1_rule(self):
        assembler = FakeAssembler(HEADLESS, REPO / "kernel" / "src" / "embedded-x86_64",
                                  seed_hex="11" * 32)
        assembler.check_isolation()  # no refusal: the private-fixture path is unchanged

    def test_copy_then_sign_keeps_the_cargo_target_unsigned(self):
        for profile in (HEADLESS, DESKTOP):
            with self.subTest(profile=profile.name):
                assembler = self.assembler(profile)
                assembler.assemble()
                (root,) = assembler.candidate_roots
                self.assertEqual(root.parent, self.out)
                self.assertTrue(root.name.startswith(".assemble-"))
                for path in assembler.signed_paths:
                    self.assertEqual(path.parent, root / "inputs", path)
                for name in {cell.binary for cell in profile.cells} | {"app-init"}:
                    self.assertEqual(assembler.binary(name).read_bytes(),
                                     b"\x7fELF built " + name.encode(), name)

    def test_real_sign_hook_routes_dev_key_and_keeps_the_seed_cli(self):
        work = self.root / "work"
        targets = [work / "inputs" / "cellbox", work / "inputs" / "app-init"]
        for profile in (HEADLESS, DESKTOP):
            with self.subTest(profile=profile.name), \
                    mock.patch.object(asm.signing_cli, "sign_route", return_value=0) as route, \
                    mock.patch.object(asm.subprocess, "run") as run:
                asm.Assembler(profile, self.out, objcopy="objcopy").sign(targets, work)
                route.assert_called_once_with(REPO, "x86-candidate-" + profile.name, work,
                                              targets, "objcopy")
                run.assert_not_called()
        with mock.patch.object(asm.signing_cli, "sign_route") as route, \
                mock.patch.object(asm.subprocess, "run") as run:
            run.return_value.returncode = 0
            asm.Assembler(HEADLESS, self.out, seed_hex="11" * 32).sign(targets, work)
            route.assert_not_called()
            command = run.call_args.args[0]
            self.assertIn("--seed-hex", command)
            self.assertEqual(command[-2:], [str(t) for t in targets])
        with mock.patch.object(asm.signing_cli, "sign_route", return_value=3):
            with self.assertRaisesRegex(asm.AssemblyError, "refused"):
                asm.Assembler(HEADLESS, self.out).sign(targets, work)

    def test_route_table_matches_the_profiles(self):
        for kind in ("candidate", "canonical"):
            for profile in (HEADLESS, DESKTOP):
                with self.subTest(kind=kind, profile=profile.name):
                    route = signing.QUALIFIED_ROUTES[f"x86-{kind}-" + profile.name]
                    self.assertEqual(route.binaries, {cell.binary for cell in profile.cells
                                                      if cell.signed} | {"app-init"})
                    self.assertEqual(route.fixture_names,
                                     route.binaries & signing.PRIVATE_FIXTURE_NAMES)
                    self.assertEqual(route.recipe, "scripts/assemble-x86-image.py")
                    self.assertEqual(route.in_repo_root,
                                     None if kind == "candidate" else signing.X86_CANONICAL_ROOT)
        self.assertEqual(set(signing.QUALIFIED_ROUTES),
                         {"x86-candidate-headless", "x86-candidate-desktop",
                          "x86-canonical-headless", "x86-canonical-desktop"})


class CommandLine(unittest.TestCase):
    def test_seed_file_must_be_a_private_32_byte_seed(self):
        with tempfile.TemporaryDirectory() as tmp:
            seed = Path(tmp) / "seed.hex"
            for content, pattern in ((signing.DEV_SEED_HEX, "dev seed"), ("abcd", "64 hex"),
                                     ("zz" * 32, "64 hex")):
                with self.subTest(content=content[:8]):
                    seed.write_text(content + "\n")
                    with self.assertRaisesRegex(asm.AssemblyError, pattern):
                        asm.read_seed_file(seed)
            seed.write_text("AB" * 32 + "\n")
            self.assertEqual(asm.read_seed_file(seed), "ab" * 32)

    def test_test_inject_may_not_replace_a_manifest_path(self):
        for path in ("/bin/vfs", "/bin/cellbox-ssh", "/POLICY.BIN", "/etc/cellos/autoload.toml"):
            with self.subTest(path=path):
                with self.assertRaisesRegex(SystemExit, "outside the manifest"):
                    asm.main(["--profile", "headless", "--out-dir", "/nonexistent",
                              "--test-inject", "/dev/null=" + path])

    def test_assembler_never_sets_the_signing_allowances(self):
        text = (REPO / "scripts" / "assemble-x86-image.py").read_text()
        code = text.split('"""', 2)[2]  # skip the docstring that names them
        for name in ("CELLBOX_PRIVATE_FIXTURE_SIGNING", "CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI"):
            self.assertNotIn(name, code)


if __name__ == "__main__":
    unittest.main()
