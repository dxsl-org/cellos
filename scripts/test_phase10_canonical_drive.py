#!/usr/bin/env python3
"""Host-only H1/H2/H3 and L0 stream tests. No Docker, QEMU, cargo build, or network.

    python3 -B scripts/test_phase10_canonical_drive.py
"""

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import phase10_canonical_drive as drive
import phase10_l0_stream as stream

REPO = Path(__file__).resolve().parent.parent
FIXTURE = REPO / "scripts/phase09_ssh_fixture.sh"
RUNNER = REPO / "scripts/qemu-phase10-canonical-test.sh"


class PromptTests(unittest.TestCase):
    def shell(self, raw):
        return drive.Shell(SimpleNamespace(serial=SimpleNamespace(raw=lambda: raw), alive=lambda: True))

    def test_services_and_kernel_lines_after_empty_prompt_are_idle_and_retained(self):
        raw = (b"USER: Cellos > echo ok\nUSER: ok\nUSER: Cellos > \n"
               b"[ INFO] kernel background\nUSER: [net-bridge] NIC timeout\n"
               b"USER: [serial] ready\nUSER: [nvme] slow completion io opc=0 ms=1200\n")
        sh = self.shell(raw)
        self.assertTrue(sh.idle(count=2))
        self.assertTrue(sh.wait_prompt(2, 1))
        self.assertEqual(sh.raw(), raw)
        lines = drive.logical_lines(raw)
        self.assertIn("[net-bridge] NIC timeout", lines)
        self.assertIn("[serial] ready", lines)
        self.assertIn("[nvme] slow completion io opc=0 ms=1200\n", lines)

    def test_command_output_after_prompt_is_not_idle(self):
        for tail in (b"USER: command output\n", b"USER: [Net-bridge] not a service tag\n",
                     b"USER: [net_bridge] not a service tag\n", b"USER: [serial]missing space\n",
                     b"USER: command output\nUSER: [serial] background\n"):
            with self.subTest(tail=tail):
                self.assertFalse(self.shell(b"USER: Cellos > \n" + tail).idle())

    def test_prompt_substring_is_not_an_empty_prompt(self):
        for line in (b"USER: output mentions Cellos > \n", b"USER: not a Cellos > prompt\n",
                     b"USER: Cellos > running\n", b"USER: output Cellos > \nUSER: [serial] ready\n"):
            with self.subTest(line=line):
                self.assertFalse(self.shell(line).idle())
        self.assertEqual(self.shell(b"USER: output mentions Cellos > \n").prompts(), 0)

    def test_service_lines_without_prompt_do_not_make_idle(self):
        self.assertFalse(self.shell(b"USER: [serial] ready\n[ INFO] background\n").idle())

    def test_empty_prompt_requires_increased_count(self):
        raw = b"USER: Cellos > \nUSER: [serial] ready\n"
        sh = self.shell(raw)
        self.assertTrue(sh.idle())
        self.assertFalse(sh.idle(count=sh.prompts() + 1))
        with mock.patch.object(drive.time, "time", side_effect=[0, 0, 2]), mock.patch.object(drive.time, "sleep"):
            self.assertFalse(sh.wait_prompt(2, 1))
        self.assertTrue(self.shell(raw + b"USER: Cellos > ").idle(count=2))

    def test_ansi_and_kernel_interrupted_prompt(self):
        raw = b"USER: \x1b[0mCellos > \n[ WARN] background\nUSER+ \x1b[0m"
        self.assertTrue(self.shell(raw).idle(count=1))

    def test_tui_restoration_frames_the_unprefixed_shell_prompt(self):
        for restore in (b"\x1b[?1049l\x1b[?25h\x1b[?2004l",
                        b"\x1b[?1049l\x1b[?2004l"):
            with self.subTest(restore=restore):
                running = (b"USER: Cellos > top\nUSER: \x1b[?1049h"
                           b"\x1b[1;1Hrendered screen")
                returned = running + restore + b"Cellos > "
                self.assertEqual(self.shell(running).prompts(), 1)
                self.assertFalse(self.shell(running).idle(count=2))
                sh = self.shell(returned)
                self.assertEqual(sh.prompts(), 2)
                self.assertTrue(sh.wait_prompt(2, 1))
                self.assertFalse(sh.idle(count=3))
                self.assertTrue(self.shell(returned + b"\nUSER: [serial] ready\n").idle(count=2))
                self.assertFalse(self.shell(returned + b"echo running").idle(count=2))
                self.assertEqual(sh.raw(), returned)

    def test_tui_controls_without_restoration_do_not_frame_output_as_prompt(self):
        for control in (b"", b"\x1b[0m", b"\x1b[?25h", b"\x1b[?1049h"):
            with self.subTest(control=control):
                raw = b"USER: Cellos > cat file\nUSER: arbitrary text" + control + b"Cellos > "
                self.assertEqual(self.shell(raw).prompts(), 1)
                self.assertFalse(self.shell(raw).idle(count=2))

    def test_run_removes_actual_prompt_not_the_last_service_line(self):
        raw = b"echo ok\nUSER: ok\nUSER: Cellos > \nUSER: [serial] ready\n"
        sh = self.shell(raw)
        with mock.patch.object(sh, "submit", return_value=(0, 0)):
            output, segment = sh.run("echo ok", timeout=1)
        self.assertEqual(output, ["ok", "[serial] ready\n"])
        self.assertEqual(segment, raw)


# Real shell scripts are driven through fake executables, not static source assertions.
DOCKER = r'''import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with open(os.environ["DOCKER_CALLS"], "a") as out:
    out.write(json.dumps(args) + "\n")
stage = os.environ.get("FAIL_STAGE", "")
if args[:2] == ["image", "inspect"]:
    missing = os.environ.get("IMAGE_MISSING") and not Path(os.environ["BUILD_RECIPE"]).exists()
    raise SystemExit(1 if missing or stage == "fixture-build" else 0)
if args and args[0] == "build":
    Path(os.environ["BUILD_RECIPE"]).write_text(sys.stdin.read())
    raise SystemExit(1 if stage == "fixture-build" else 0)
if args and args[0] == "run":
    raise SystemExit(1 if stage == "fixture-start" else 0)
if args and args[0] == "logs":
    print("Server listening on 0.0.0.0 port 22.")
    print("Server listening on 0.0.0.0 port 2222.")
if args and args[0] == "port":
    print("127.0.0.1:" + ("40022" if args[-1] == "22/tcp" else "42222"))
if args and args[0] == "exec":
    print("OpenSSH_fixture_test")
'''
KEYGEN = r'''import sys
from pathlib import Path
args = sys.argv[1:]
if "-l" in args:
    print("256 SHA256:fixture-test p09-fixture (ED25519)")
else:
    key = Path(args[args.index("-f") + 1])
    key.write_text("PRIVATE fixture test key\n")
    Path(str(key) + ".pub").write_text("ssh-ed25519 fixture-test\n")
'''
PREPARATION = r'''import json, os, sys
from pathlib import Path
args = sys.argv[1:]
root = Path(os.environ["FAKE_ROOT"])
fail = os.environ.get("FAIL_STAGE", "")
def stage(name):
    with open(root / "stages", "a") as out:
        out.write(name + "\n")
    if fail == name:
        raise SystemExit(1 if name == "driver-after-launch" else 2)
if args[0] == "-":
    sys.stdin.read()
    if len(args) == 2:
        stage("work-validation")
    elif "@@" in args[1]:
        stage("data-readback")
    elif len(args) == 4:
        stage("input-verification")
        Path(args[3]).write_text(json.dumps({"iso": {"path": str(root / "candidate.iso")}, "boot_config": str(root / "boot-config")}))
    else:
        stage("provenance")
elif args[0] == "-c":
    print(root / ("candidate.iso" if '["iso"]' in args[1] else "boot-config"))
elif args[0].endswith("phase07_rg_fixture.py"):
    stage("main-disk")
    Path(args[1]).write_bytes(b"main disk fixture")
    Path(args[2]).write_text("{}")
elif args[0].endswith("phase09_trust_disk.py"):
    stage("trust-disk")
    Path(args[1]).write_bytes(b"ssh disk fixture")
elif args[0].endswith("phase10_canonical_drive.py"):
    if args[1] == "fixtures":
        stage("data-fixtures")
        tree = Path(args[2])
        tree.mkdir(parents=True)
        (tree / "test.txt").write_text("test fixture")
        Path(args[args.index("--manifest") + 1]).write_text("{}")
    else:
        stage("driver-before-launch")
        evidence = Path(args[args.index("--evidence") + 1])
        (evidence / "qemu-started").write_text("main\n")
        stage("driver-after-launch")
else:
    raise SystemExit("unexpected fake preparation call: " + repr(args))
'''


class ExecutableFixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        DOCKER_CALLS=str(self.root / "docker-calls"), BUILD_RECIPE=str(self.root / "build-recipe"),
                        FAKE_ROOT=str(self.root), PYTHONDONTWRITEBYTECODE="1")
        self.env.pop("IMAGE_MISSING", None)
        self.env.pop("FAIL_STAGE", None)
        self.executable("docker", DOCKER)
        self.executable("ssh-keygen", KEYGEN)

    def tearDown(self):
        self.tmp.cleanup()

    def executable(self, name, code):
        path = self.bin / name
        path.write_text(f"#!{sys.executable}\n" + code)
        path.chmod(0o755)
        return path

    def calls(self):
        path = self.root / "docker-calls"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


class ImageRecipeTests(ExecutableFixture):
    def command(self, command, script=FIXTURE):
        return subprocess.check_output(["bash", str(script), command], env=self.env)

    def test_tag_is_full_sha256_of_exact_pinned_build_recipe(self):
        recipe = self.command("recipe")
        tag = self.command("image").decode().strip()
        self.assertEqual(tag, "cellbox-p09-sshd:" + hashlib.sha256(recipe).hexdigest())
        self.assertRegex(recipe.decode(), r"^FROM alpine@sha256:[0-9a-f]{64}\n")
        self.assertIn(b"apk add --no-cache -q openssh-server", recipe)
        self.assertEqual(self.command("image"), self.command("image"))
        self.assertEqual(self.calls(), [])

    def test_base_or_package_change_changes_tag(self):
        source = FIXTURE.read_text()
        original = self.command("image")
        for name, edited in (("base", source.replace("ce64758a", "ae64758a")),
                             ("packages", source.replace("-q openssh-server", "-q openssh-server openssh-client"))):
            script = self.root / (name + ".sh")
            script.write_text(edited)
            with self.subTest(recipe=name):
                recipe = self.command("recipe", script)
                self.assertNotEqual(self.command("image", script), original)
                self.assertEqual(self.command("image", script).decode().strip().split(":", 1)[1],
                                 hashlib.sha256(recipe).hexdigest())

    def start(self, name):
        work = self.root / name
        work.mkdir()
        result = subprocess.run(["bash", str(FIXTURE), "start", str(work)], env=self.env,
                                text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return work

    def test_cached_start_never_builds_pulls_or_installs_packages(self):
        work = self.start("cached")
        calls = self.calls()
        self.assertFalse(any(args[0] in ("build", "pull") for args in calls))
        run = next(args for args in calls if args[0] == "run")
        self.assertIn("--pull=never", run)
        self.assertNotIn("apk", " ".join(run))
        self.assertEqual((work / "sshd-version").read_text().strip(), "OpenSSH_fixture_test")
        self.assertEqual((work / "port").read_text().strip(), "40022")
        self.assertEqual((work / "rekey_port").read_text().strip(), "42222")
        self.assertIn("PerSourcePenalties no", (work / "sshd_config").read_text())
        self.assertIn("RekeyLimit 64K", (work / "sshd_config_rekey").read_text())
        self.assertTrue((work / "p9").is_file())

    def test_missing_image_builds_exact_recipe_before_container_start(self):
        self.env["IMAGE_MISSING"] = "1"
        self.start("build")
        calls = self.calls()
        build = next(i for i, args in enumerate(calls) if args[0] == "build")
        launch = next(i for i, args in enumerate(calls) if args[0] == "run")
        self.assertLess(build, launch)
        self.assertEqual((self.root / "build-recipe").read_bytes(), self.command("recipe"))
        self.start("reuse")
        self.assertEqual(sum(args[0] == "build" for args in self.calls()), 1)


class PrebootStatusTests(ExecutableFixture):
    def setUp(self):
        super().setUp()
        self.env["PYTHON_BIN"] = str(self.executable("prepare-python", PREPARATION))
        self.executable("mcopy", "import os\nraise SystemExit(2 if os.environ.get('FAIL_STAGE') == 'mcopy' else 0)\n")
        self.executable("cargo", "raise SystemExit(2)\n")
        cp = shutil.which("cp")
        self.executable("cp", f"import os, sys\nif os.environ.get('FAIL_STAGE') == 'fixture-metadata' and any('host_ed25519.pub' in a for a in sys.argv[1:]):\n    raise SystemExit(2)\nos.execv({cp!r}, [{cp!r}, *sys.argv[1:]])\n")
        (self.root / "candidate.json").write_text("{}")
        (self.root / "candidate.iso").write_bytes(b"not booted")
        (self.root / "boot-config").mkdir()

    def run_wrapper(self, stage, profile="headless"):
        self.env["FAIL_STAGE"] = stage
        with tempfile.TemporaryDirectory(prefix="cellbox-phase10-", dir="/tmp") as work:
            result = subprocess.run(["bash", str(RUNNER), "--candidate", str(self.root / "candidate.json"),
                                     "--profile", profile, "--work", work], cwd=REPO, env=self.env,
                                    text=True, capture_output=True)
            booted = (Path(work) / "evidence/qemu-started").exists()
            return result, booted

    def test_all_prelaunch_preparation_failures_are_uncounted(self):
        for stage in ("work-validation", "input-verification", "main-disk", "data-fixtures", "mcopy",
                      "data-readback", "fixture-build", "fixture-start", "fixture-metadata", "trust-disk",
                      "driver-before-launch"):
            with self.subTest(stage=stage):
                result, booted = self.run_wrapper(stage)
                self.assertEqual(result.returncode, 3, result.stdout + result.stderr)
                self.assertIn("candidate not booted", result.stderr)
                self.assertFalse(booted)
        result, booted = self.run_wrapper("glyph-build", "desktop")
        self.assertEqual(result.returncode, 3, result.stdout + result.stderr)
        self.assertFalse(booted)

    def test_prevalidation_missing_candidate_is_status_three(self):
        (self.root / "candidate.json").unlink()
        result, booted = self.run_wrapper("")
        self.assertEqual(result.returncode, 3, result.stdout + result.stderr)
        self.assertFalse(booted)

    def test_failure_after_launch_remains_counted(self):
        result, booted = self.run_wrapper("driver-after-launch")
        self.assertTrue(booted, result.stdout + result.stderr)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertNotIn("candidate not booted", result.stderr)

    def test_success_and_usage_keep_their_exit_status(self):
        result, booted = self.run_wrapper("")
        self.assertTrue(booted, result.stdout + result.stderr)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result = subprocess.run(["bash", str(RUNNER), "--profile", "invalid"], env=self.env, capture_output=True)
        self.assertEqual(result.returncode, 2)

    def test_machine_records_launch_only_after_successful_popen(self):
        machine = drive.Machine.__new__(drive.Machine)
        machine.serial_path = self.root / "serial.sock"
        machine.qmp_path = self.root / "qmp.sock"
        machine.a = SimpleNamespace(evidence=str(self.root))
        machine.label = "main"
        machine.argv = ["not-a-real-qemu"]
        with mock.patch.object(drive.subprocess, "Popen", side_effect=OSError("launch failed")):
            with self.assertRaises(OSError):
                machine.start()
        self.assertFalse((self.root / "qemu-started").exists())
        with mock.patch.object(drive.subprocess, "Popen"), mock.patch.object(drive, "Serial"), mock.patch.object(drive, "Qmp"):
            machine.start()
        self.assertEqual((self.root / "qemu-started").read_text(), "main\n")


class StreamTests(unittest.TestCase):
    def records(self, statuses, profile="headless"):
        return [{"lane": f"L0-{profile}-run{i}", "exit": status} for i, status in enumerate(statuses, 1)]

    def test_status_three_neither_counts_nor_changes_streak(self):
        state = stream.evaluate(self.records([0, 0, 3, 3, 0, 1, 3, 0]), ["headless"])
        headless = state["profiles"]["headless"]
        self.assertEqual(headless["counted_runs"], 5)
        self.assertEqual(headless["uncounted"], 3)
        self.assertEqual(headless["streak"], 1)
        self.assertEqual(headless["longest_streak"], 3)
        self.assertFalse(state["infrastructure_blocked"])

    def test_three_consecutive_infrastructure_failures_stop_stream(self):
        state = stream.evaluate(self.records([0, 3, 3, 3]), ["headless"])
        self.assertTrue(state["infrastructure_blocked"])
        self.assertEqual(state["profiles"]["headless"]["counted_runs"], 1)
        self.assertEqual(state["profiles"]["headless"]["streak"], 1)
        self.assertFalse(stream.evaluate(self.records([3, 3, 1, 3]), ["headless"])["infrastructure_blocked"])

    def test_twelve_counted_runs_bound_ignores_status_three(self):
        statuses = [0, 1, 3] * 6
        state = stream.evaluate(self.records(statuses), ["headless"])
        self.assertEqual(state["profiles"]["headless"]["counted_runs"], 12)
        self.assertEqual(state["exhausted"], ["headless"])
        self.assertFalse(state["infrastructure_blocked"])

    def test_dry_run_uses_fake_summary_without_modification(self):
        with tempfile.TemporaryDirectory() as tmp:
            summary = Path(tmp) / "summary.jsonl"
            data = "".join(json.dumps(record) + "\n" for record in self.records([0, 3, 0, 3, 0, 0, 0, 0]))
            summary.write_text(data)
            result = subprocess.run([sys.executable, "-B", str(REPO / "scripts/phase10_l0_stream.py"),
                                     "--summary", str(summary), "--profiles", "headless", "--dry-run"],
                                    text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            state = json.loads(result.stdout)
            self.assertTrue(state["complete"])
            self.assertEqual(state["profiles"]["headless"]["counted_runs"], 6)
            self.assertEqual(summary.read_text(), data)

    def run_stream(self, statuses, required=6, max_runs=12):
        with tempfile.TemporaryDirectory() as tmp:
            runner = Path(tmp) / "run_lane.py"
            summary = Path(tmp) / "summary.jsonl"
            calls = []
            iterator = iter(statuses)
            def record(command, cwd):
                calls.append(command)
                status = next(iterator)
                with summary.open("a") as out:
                    out.write(json.dumps({"lane": command[3], "exit": status}) + "\n")
                return status
            status = stream.orchestrate("candidate.json", runner, ["headless"], required, max_runs,
                                        invoke=record, report=lambda state: None)
            return status, calls, stream.read_summary(summary)

    def test_orchestrator_retries_infrastructure_then_stops_at_six(self):
        status, calls, records = self.run_stream([0, 0, 3, 3, 0, 0, 0, 0])
        self.assertEqual(status, 0)
        self.assertEqual(len(calls), 8)
        self.assertEqual([r["lane"] for r in records], [f"L0-headless-run{i}" for i in range(1, 9)])
        self.assertIn("{work}", calls[-1])

    def test_orchestrator_stops_immediately_at_three_infrastructure_failures(self):
        status, calls, records = self.run_stream([0, 3, 3, 3, 0])
        self.assertEqual(status, 3)
        self.assertEqual(len(calls), 4)
        self.assertEqual(stream.evaluate(records, ["headless"])["profiles"]["headless"]["streak"], 1)

    def test_orchestrator_stops_at_counted_bound(self):
        status, calls, records = self.run_stream([0, 1, 3] * 6)
        self.assertEqual(status, 1)
        self.assertEqual(len(calls), 17)
        self.assertEqual(stream.evaluate(records, ["headless"])["profiles"]["headless"]["counted_runs"], 12)

    def test_recorder_failure_never_fabricates_summary(self):
        with tempfile.TemporaryDirectory() as tmp:
            runner = Path(tmp) / "run_lane.py"
            with self.assertRaisesRegex(RuntimeError, "did not append exactly one matching record"):
                stream.orchestrate("candidate.json", runner, ["headless"], invoke=lambda *a, **kw: 1,
                                   report=lambda state: None)
            self.assertFalse((Path(tmp) / "summary.jsonl").exists())

    def test_resume_uses_recorded_attempt_numbers_and_independent_profiles(self):
        records = self.records([0, 3, 0]) + self.records([1, 0], "desktop")
        records.append({"lane": "L4-run1", "exit": 3})
        state = stream.evaluate(records)
        self.assertEqual(state["profiles"]["headless"]["next_attempt"], 4)
        self.assertEqual(state["profiles"]["headless"]["streak"], 2)
        self.assertEqual(state["profiles"]["desktop"]["streak"], 1)


if __name__ == "__main__":
    unittest.main()
