#!/usr/bin/env python3
"""Cellos x86 scenarios on VMware Workstation, driven from WSL2.

Subcommands:
  fault (default)  the native AHCI storage/fault matrix (four scenarios, file serial).
  lane             one NIC x storage x vCPU x firmware cell: pipe serial through
                   vmware_serial_relay.py, typed commands, two-boot persistence.
  v0               the S3 V0 tooling smoke: the lane cells and host probes of V0_SCENARIOS.
  matrix           the phase-09 sweep (matrix_scenarios): {e1000, e1000e, vmxnet3} x
                   {sata, nvme, pvscsi} x {2, 4 vCPU} x {bios, efi}, two boots each, plus
                   one `pciBridge0` cell per storage type. Verdicts per cell: prompt, NIC
                   bind + DHCP lease, two-boot persistence with host read-back, guest S5.

The lane creates every VM under LANE_ROOT (C:\\Temp\\cellos-vmware\\s3-lane), one
directory per scenario, and deletes it (`vmrun deleteVM` + removal) after the
run. Retained evidence goes to build/x86-backend-evidence/vmware-lane/<run>/.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import fnmatch
import hashlib
import importlib.util
import itertools
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("vmboot", ROOT / "scripts/x86/vmware-boot-test.py")
VM = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VM)

def serial_lines(raw):
    """Stripped Cell/kernel lines of a serial capture, `USER: ` removed. A `USER+ ` line
    is the rest of a Cell line a kernel log line interrupted (kernel `ConsoleLine`
    contract): it is joined onto that Cell line first instead of standing alone."""
    joined, last_cell = [], None
    for line in raw.decode(errors="replace").splitlines():
        if line.startswith("USER+ ") and last_cell is not None:
            joined[last_cell] += line[len("USER+ "):]
            continue
        if line.startswith(("USER: ", "USER+ ")):
            last_cell = len(joined)
            line = "USER: " + line[len("USER+ "):]
        joined.append(line)
    return [line.strip().removeprefix("USER: ") for line in joined if line.strip()]

def ahci_ready(line):
    match = re.fullmatch(
        r"\[driver_cell\] ahci storage driver ready \(port=([0-9]{1,2}) sectors=([1-9][0-9]{0,19}) identify ok\)",
        line)
    return bool(match and int(match[1]) < 32 and int(match[2]) <= (1 << 64) - 1)

def sha256(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def raw_disk_sha256(disk):
    """Hash guest-visible bytes, excluding mutable VMDK container metadata."""
    with tempfile.TemporaryDirectory(prefix="cellos-native-disk-check-") as directory:
        raw = Path(directory) / "disk.raw"
        subprocess.run(["qemu-img", "convert", "-f", "vmdk", "-O", "raw",
                        str(disk), str(raw)], check=True, capture_output=True)
        return sha256(raw)

def corrupt_p5_superblocks(source, target):
    """Offline fixture: invalidate both 4KiB roots of P5, not merely root A."""
    shutil.copyfile(source, target)
    with Path(target).open("r+b") as stream:
        stream.seek(931072 * 512)
        if stream.write(bytes(2 * 4096)) != 2 * 4096:
            raise RuntimeError("incomplete P5 superblock corruption")

def make_native_vmx(iso, serial, disk=None):
    return VM.make_vmx(VM.to_host_path(iso), VM.to_host_path(serial), 1, 256,
                       storage="sata" if disk else None,
                       disk=VM.to_host_path(disk) if disk else None,
                       name="Cellos native storage qualification")

def run_native_boot(vmrun, workspace, name, iso, disk=None, window=90):
    run_dir = workspace / name
    run_dir.mkdir()
    serial = run_dir / "serial.log"
    vmx = run_dir / "cellos.vmx"
    vmx_content = make_native_vmx(iso, serial, disk)
    vmx.write_text(vmx_content)
    (run_dir / "requested.vmx").write_text(vmx_content)

    res = {
        "scenario": name,
        "disk": str(disk) if disk else None,
        "disk_sha256": sha256(disk) if disk else None,
        "status": "FAIL",
        "errors": [],
        "cleanup": False,
        "markers": [],
    }

    started = False
    terminal_observed = False
    try:
        if name == "native-ahci-corrupt-refusal":
            if disk is None:
                raise RuntimeError("corrupt-refusal requires a disk fixture")
            res["disk_raw_sha256_before"] = raw_disk_sha256(disk)
        command = [str(vmrun), "-T", "ws", "start", VM.to_host_path(vmx), "nogui"]
        started = True
        code = VM.run_logged(command, run_dir / "vmrun-start.log", 30)
        if code != 0:
            raise RuntimeError(f"vmrun start failed (code {code})")

        deadline = time.monotonic() + window
        while time.monotonic() < deadline:
            raw = VM.read_serial(serial)
            if time.monotonic() >= deadline:
                break
            if "Cellos >" in serial_lines(raw) or b"panic" in raw.lower():
                terminal_observed = True
                break
            time.sleep(0.5)
        if not terminal_observed:
            raise TimeoutError(f"no terminal boot marker within {window}s")

    except Exception as exc:
        res["errors"].append(str(exc))
    finally:
        if started:
            try:
                VM.stopped_vm(vmrun, vmx, run_dir, 30)
                res["cleanup"] = True
            except Exception as exc:
                res["errors"].append(f"cleanup: {exc}")

        raw = VM.read_serial(serial)
        res["serial_sha256"] = hashlib.sha256(raw).hexdigest()
        lines = serial_lines(raw)
        res["markers"] = [l for l in lines if any(k in l.lower() for k in ["ahci", "block_driver", "cellosfs", "[vfs]", "panic", "cellos >"])]

        has_shell = "Cellos >" in res["markers"]
        has_ahci = any(ahci_ready(m) for m in res["markers"])
        mount_count = res["markers"].count("[vfs] CellosFS Native mounted at existing volume")
        refusal_count = res["markers"].count("[vfs] WARNING: CellosFS open failed — volume unavailable")
        has_panic = any("panic" in m.lower() for m in res["markers"])
        if not has_shell or has_panic:
            res["errors"].append(f"boot incomplete or panicked (shell={has_shell}, panic={has_panic})")
        if any("CellosFS Native formatted" in m for m in res["markers"]):
            res["errors"].append("boot unexpectedly formatted a native volume")

        if name in ("native-ahci-mount-basic", "native-ahci-mount-reboot"):
            if not has_ahci or mount_count != 2 or refusal_count != 0:
                res["errors"].append(f"missing healthy storage (ahci={has_ahci}, mounts={mount_count}, refusals={refusal_count})")
        elif name == "native-ahci-no-disk":
            if mount_count != 0:
                res["errors"].append("no-disk scenario unexpectedly mounted a native volume")
        elif name == "native-ahci-corrupt-refusal":
            if not has_ahci or mount_count != 1 or refusal_count != 1:
                res["errors"].append(f"missing exact refusal (ahci={has_ahci}, mounts={mount_count}, refusals={refusal_count})")
            if res["cleanup"] and "disk_raw_sha256_before" in res:
                try:
                    res["disk_raw_sha256_after"] = raw_disk_sha256(disk)
                    if res["disk_raw_sha256_after"] != res["disk_raw_sha256_before"]:
                        res["errors"].append("corrupt-refusal modified guest-visible disk bytes")
                except Exception as exc:
                    res["errors"].append(f"disk verification: {exc}")
        else:
            res["errors"].append(f"unknown scenario: {name}")

        if res["cleanup"] and not res["errors"]:
            res["status"] = "PASS"

        (run_dir / "result.json").write_text(json.dumps(res, indent=2) + "\n")

    return res


# --- S3 VMware lane: pipe serial, typed commands, two-boot persistence ---------

LANE_ROOT = Path("/mnt/c/Temp/cellos-vmware/s3-lane")
EVIDENCE_ROOT = ROOT / "build/x86-backend-evidence/vmware-lane"
RELAY = ROOT / "scripts/x86/vmware_serial_relay.py"
PIN_DISK = ROOT / "scripts/x86/machine_pin_disk.py"
PROMPT = "Cellos >"
PERSIST_FILE = "s3-lane.txt"
FAT_OFFSET = 2048 * 512  # machine_pin_disk.py P1 (FAT32, /mnt/sd)
CMD_TIMEOUT = 20.0
SHUTDOWN_TIMEOUT = 30.0
TYPE_DELAY = 0.005
REJECT = ("kernel panic", "panicked at", "[fault] cell", "tier 2 admission: enabled")
MARKER_KEYS = ("[x86-gate] board=", "[acpi] gates:", "[x86-smp] topology", "[driver_cell]",
               "[vfs] FAT32", "machine-pin:", "SAFE MODE", "[e1000]", "[e1000e]", "[igb]",
               "[vmxnet3]", "[virtio-net]", "[net]", "[net-bridge]", "[pcie]", "[platform]",
               "Ethernet", "class 2:0:0", "[ahci]", "[nvme]", "[pvscsi]", "[acpi] power",
               "[power]", "System shutting down", "shutdown:", "panic", "[fault]")
# Matrix NIC verdict: the exact VMware identity bound by its own cell, the
# registration with the net bridge, and a DHCP lease from VMnet8 (NAT).
NIC_BIND = {"e1000": "[e1000] controller bound 8086:100f ",
            "e1000e": "[e1000e] controller bound 8086:10d3 ",
            "vmxnet3": "[vmxnet3] controller bound 15ad:07b0 "}
NIC_REGISTERED = "[driver_cell] NIC driver registered: tid="
DHCP_LEASE = "[net] DHCP acquired"
# Every NIC cell starts; the ones without a device log `[<cell>] no … controller
# present; driver cell idle`. Only the cell's own idle line is a refusal (nic_errors).
NIC_REFUSALS = ("driver gate closed", "[platform] registration refused",
                "[platform] unsupported Ethernet identity")
DHCP_TIMEOUT = 45.0

# V0 as run: EFI cells at 512 MiB, because at >= 1024 MiB the kernel then panicked
# under VMware EFI (`efi-m1024` records it; fixed since by S3, so the matrix runs EFI
# at 1024 MiB). Probes record a host or Cellos fact and pass when they ran and were
# cleaned up.
V0_SCENARIOS = (
    {"name": "bios-sata-c2", "firmware": "bios", "storage": "sata", "cpus": 2, "boots": 2},
    {"name": "efi-sata-c2", "firmware": "efi", "storage": "sata", "cpus": 2, "boots": 2,
     "memory_mib": 512},
    {"name": "bios-nvme-c4", "firmware": "bios", "storage": "nvme", "cpus": 4, "boots": 2},
    {"name": "efi-nvme-c4", "firmware": "efi", "storage": "nvme", "cpus": 4, "boots": 2,
     "memory_mib": 512},
    {"name": "nic-e1000", "firmware": "bios", "storage": "sata", "cpus": 2, "nic": "e1000"},
    {"name": "nic-e1000e", "firmware": "bios", "storage": "sata", "cpus": 2, "nic": "e1000e"},
    {"name": "nic-vmxnet3", "firmware": "bios", "storage": "sata", "cpus": 4, "nic": "vmxnet3"},
    {"name": "efi-m1024", "firmware": "efi", "storage": "sata", "cpus": 2, "probe": True,
     "window": 60.0},
    {"name": "viommu-efi-vm", "firmware": "efi", "storage": "sata", "cpus": 2, "viommu": True,
     "memory_mib": 512, "probe": True},
    {"name": "viommu-efi-pc", "firmware": "efi", "storage": "sata", "cpus": 2, "viommu": True,
     "memory_mib": 512, "profile": "pc", "probe": True},
    {"name": "viommu-bios-vm", "firmware": "bios", "storage": "sata", "cpus": 2, "viommu": True,
     "probe": True},
    {"name": "bridge0-sata", "firmware": "bios", "storage": "sata", "cpus": 2,
     "legacy_bridge": True, "probe": True},
)

MATRIX_NICS = ("e1000", "e1000e", "vmxnet3")
MATRIX_STORAGE = ("sata", "nvme", "pvscsi")
MATRIX_CPUS = (2, 4)
MATRIX_FIRMWARE = ("bios", "efi")
# One `pciBridge0` cell per storage type (VMware's own default VMX shape): AHCI and
# the e1000 move behind the legacy bridge, the PCIe devices behind its root ports.
MATRIX_BRIDGE = (("sata", "e1000", 2, "bios"), ("nvme", "e1000e", 4, "efi"),
                 ("pvscsi", "vmxnet3", 2, "efi"))


def matrix_scenarios():
    """The phase-09 sweep: 36 grid cells plus the `pciBridge0` cells, two boots each."""
    base = {"boots": 2, "memory_mib": 1024, "net": True, "poweroff": True}
    cells = [dict(base, name=f"{nic}-{storage}-c{cpus}-{firmware}", nic=nic, storage=storage,
                  cpus=cpus, firmware=firmware)
             for nic, storage, cpus, firmware in itertools.product(
                 MATRIX_NICS, MATRIX_STORAGE, MATRIX_CPUS, MATRIX_FIRMWARE)]
    cells += [dict(base, name=f"bridge0-{storage}-{nic}-c{cpus}-{firmware}", nic=nic,
                   storage=storage, cpus=cpus, firmware=firmware, legacy_bridge=True)
              for storage, nic, cpus, firmware in MATRIX_BRIDGE]
    return cells


def select_scenarios(specs, patterns):
    """Scenarios whose name matches any fnmatch pattern (all when none given)."""
    if not patterns:
        return list(specs)
    chosen = [spec for spec in specs if any(fnmatch.fnmatchcase(spec["name"], p) for p in patterns)]
    if not chosen:
        raise LaneError("no scenario matches " + ", ".join(patterns))
    return chosen


class LaneError(Exception):
    pass


def lane_spec(raw):
    spec = {"nic": None, "storage": None, "cpus": 2, "firmware": "bios", "viommu": False,
            "legacy_bridge": False, "boots": 1, "memory_mib": 1024, "window": 120.0,
            "profile": "vm", "probe": False, "net": False, "poweroff": False}
    spec.update(raw)
    if spec["boots"] not in (1, 2) or (spec["boots"] == 2 and not spec["storage"]):
        raise LaneError("two boots need storage; boots must be 1 or 2")
    if spec["net"] and spec["nic"] not in NIC_BIND:
        raise LaneError("a NIC verdict needs one of " + ", ".join(NIC_BIND))
    return spec


class RelaySerial:
    """The guest UART through vmware_serial_relay.py: captured bytes and typed input."""

    def __init__(self, relay, pipe, log_path):
        self.data = bytearray()
        self.status = []
        self.lock = threading.Lock()
        self.log = log_path.open("wb")
        self.process = subprocess.Popen(["py.exe", "-3", relay, pipe], cwd=str(LANE_ROOT),
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE)
        self.threads = [threading.Thread(target=self._pump_out, daemon=True),
                        threading.Thread(target=self._pump_err, daemon=True)]
        for thread in self.threads:
            thread.start()

    def _pump_out(self):
        while chunk := self.process.stdout.read1(4096):
            with self.lock:
                self.data += chunk
                self.log.write(chunk)
                self.log.flush()

    def _pump_err(self):
        for line in self.process.stderr:
            self.status.append(line.decode(errors="replace").rstrip())

    def connected(self):
        return any(line.startswith("relay: connected") for line in self.status)

    def mark(self):
        with self.lock:
            return len(self.data)

    def raw(self, start=0):
        with self.lock:
            return bytes(self.data[start:])

    def wait_for(self, marker, start, deadline):
        """True only when `marker` arrived after byte `start` and before `deadline`."""
        needle = marker.encode()
        while time.monotonic() < deadline:
            if needle in self.raw(start):
                return True
            time.sleep(0.1)
        return False

    def type_line(self, line):
        if self.process.poll() is not None:
            raise LaneError("relay exited before input: " + "; ".join(self.status))
        try:
            for byte in (line + "\n").encode():
                self.process.stdin.write(bytes([byte]))
                self.process.stdin.flush()
                time.sleep(TYPE_DELAY)
        except OSError as exc:
            raise LaneError("relay input failed: " + str(exc)) from exc

    def run(self, command, timeout=CMD_TIMEOUT):
        """Type one shell command; its serial up to the next prompt."""
        start = self.mark()
        self.type_line(command)
        if not self.wait_for(PROMPT, start, time.monotonic() + timeout):
            raise LaneError(f"no prompt within {timeout}s after {command!r}")
        return self.raw(start)

    def close(self):
        try:
            self.process.stdin.close()
        except OSError:
            pass
        if not self.connected() and self.process.poll() is None:
            # The VM never served the pipe (refused power-on): nothing to drain.
            self.process.kill()
            self.status.append("harness: relay stopped; pipe never connected")
        try:
            self.process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
            self.status.append("harness: relay killed after 15s")
        for thread in self.threads:
            thread.join(timeout=5)
        self.log.close()


def printed_lines(raw, command):
    """Lines a shell command printed: no echo, kernel/log lines or prompt."""
    lines = []
    for line in serial_lines(raw):
        line = line.replace(PROMPT, "").strip()
        if line and command not in line and not line.startswith("[") and not line.startswith("Init:"):
            lines.append(line)
    return lines


def vm_listed(vmrun, vmx, log):
    code = VM.run_logged([str(vmrun), "-T", "ws", "list"], log, 30)
    lines = log.read_text(encoding="utf-8", errors="replace").splitlines()
    if code != 0 or not lines or not re.fullmatch(r"Total running VMs: \d+", lines[0].strip()):
        raise LaneError("vmrun list failed; see " + log.name)
    own = VM.to_host_path(vmx).replace("\\", "/").lower()
    return any(line.strip().replace("\\", "/").lower() == own for line in lines[1:])


def make_lane_disk(vdisk, storage, run_dir, evidence):
    """machine_pin_disk.py (P1 FAT32 + blank P5 /srv) -> VMDK -> vmware-vdiskmanager."""
    seed = run_dir / "seed.vmdk"
    disk = run_dir / "disk.vmdk"
    adapter = "ide" if storage == "sata" else "lsilogic"
    with tempfile.TemporaryDirectory(prefix="cellos-vmware-lane-disk-") as directory:
        raw = Path(directory) / "disk.img"
        made = subprocess.run([sys.executable, str(PIN_DISK), str(raw)], capture_output=True, text=True)
        (evidence / "disk-machine-pin.log").write_text(made.stdout + made.stderr)
        if made.returncode != 0:
            raise LaneError("machine_pin_disk.py failed; see disk-machine-pin.log")
        subprocess.run(["qemu-img", "convert", "-f", "raw", "-O", "vmdk", "-o",
                        "subformat=monolithicSparse,adapter_type=" + adapter, str(raw), str(seed)],
                       check=True, capture_output=True)
    code = VM.run_logged([str(vdisk), "-r", VM.to_host_path(seed), "-t", "0", VM.to_host_path(disk)],
                         evidence / "vdiskmanager-convert.log", 300)
    seed.unlink()
    if code != 0 or not disk.is_file():
        raise LaneError(f"vmware-vdiskmanager -r failed (exit {code}); see vdiskmanager-convert.log")
    return disk


def host_read(disk, name):
    """The guest file `/mnt/sd/<name>` read on the host from the powered-off VMDK."""
    with tempfile.TemporaryDirectory(prefix="cellos-vmware-lane-read-") as directory:
        raw = Path(directory) / "disk.raw"
        subprocess.run(["qemu-img", "convert", "-f", "vmdk", "-O", "raw", str(disk), str(raw)],
                       check=True, capture_output=True)
        out = subprocess.run(["mtype", "-i", f"{raw}@@{FAT_OFFSET}", "::/" + name],
                             capture_output=True, env={"MTOOLS_SKIP_CHECK": "1", "PATH": "/usr/bin:/bin"})
    if out.returncode != 0:
        return None
    return out.stdout.decode(errors="replace").strip()


def vmware_facts(log_text):
    facts = {}
    product = re.search(r"Log for VMware Workstation[^\r\n]*", log_text)
    if product:
        facts["product"] = product.group(0)
    hw = re.search(r'virtualHW\.version = "(\d+)"', log_text)
    if hw:
        facts["virtual_hw_version"] = hw.group(1)
    facts["notable"] = [line.strip() for line in log_text.splitlines()
                        if re.search(r"vvtd|iommu|Msg_Post|Msg_Question|PowerOn.*fail|"
                                     r"not supported|unsupported|ethernet0\.virtualDev|"
                                     r"hypervisor|monitor mode|ULM|VMX_PowerOn", line, re.I)][:60]
    return facts


def lane_boot(vmrun, spec, run_dir, evidence, index, actions):
    """One power cycle: start, wait for the prompt, run `actions`, power off."""
    rec = {"boot": index, "errors": [], "vmrun_start_exit": None, "prompt_seconds": None,
           "guest_poweroff": False, "hard_stop": False, "stopped": False, "commands": []}
    pipe = rf"\\.\pipe\cellos-{run_dir.name}-b{index}"
    vmx = run_dir / "cellos.vmx"
    disk = run_dir / "disk.vmdk"
    text = VM.make_vmx(VM.to_host_path(run_dir / "cellos.iso"), pipe, spec["cpus"], spec["memory_mib"],
                       firmware=spec["firmware"], nic=spec["nic"], storage=spec["storage"],
                       disk=VM.to_host_path(disk) if spec["storage"] else None, pipe=True,
                       viommu=spec["viommu"], legacy_bridge=spec["legacy_bridge"],
                       name="Cellos S3 lane " + spec["name"])
    vmx.write_text(text, encoding="utf-8")
    (evidence / f"boot{index}-requested.vmx").write_text(text, encoding="utf-8")
    serial = RelaySerial(VM.to_host_path(run_dir / RELAY.name), pipe, evidence / f"boot{index}-serial.log")
    started = time.monotonic()
    prompt = False
    try:
        code = VM.run_logged([str(vmrun), "-T", "ws", "start", VM.to_host_path(vmx), "nogui"],
                             evidence / f"boot{index}-vmrun-start.log", 120)
        rec["vmrun_start_exit"] = code
        if code != 0:
            message = (evidence / f"boot{index}-vmrun-start.log").read_text(errors="replace").strip()
            raise LaneError(f"vmrun start failed (exit {code}): {message}")
        prompt = serial.wait_for(PROMPT, 0, started + spec["window"])
        if not prompt:
            raise LaneError(f"'{PROMPT}' not observed within {spec['window']}s of vmrun start")
        rec["prompt_seconds"] = round(time.monotonic() - started, 1)
        for action in actions:
            action(serial, rec)
    except (LaneError, OSError, subprocess.SubprocessError) as exc:
        rec["errors"].append(str(exc))
    finally:
        shutdown_typed = False
        if prompt and serial.process.poll() is None:
            # S2 S5 power-off; durability does not rest on it (FAT writes are
            # written through), a hard stop follows when it does not happen.
            try:
                serial.type_line("/bin/shutdown")
                shutdown_typed = True
                deadline = time.monotonic() + SHUTDOWN_TIMEOUT
                while time.monotonic() < deadline and vm_listed(vmrun, vmx, evidence / "vmrun-list.log"):
                    time.sleep(1)
            except (LaneError, OSError, subprocess.SubprocessError) as exc:
                rec["errors"].append("guest shutdown: " + str(exc))
        try:
            if vm_listed(vmrun, vmx, evidence / "vmrun-list.log"):
                rec["hard_stop"] = True
                VM.run_logged([str(vmrun), "-T", "ws", "stop", VM.to_host_path(vmx), "hard"],
                              evidence / f"boot{index}-vmrun-stop.log", 60)
            else:
                rec["guest_poweroff"] = shutdown_typed
            rec["stopped"] = not vm_listed(vmrun, vmx, evidence / "vmrun-list.log")
        except (LaneError, OSError, subprocess.SubprocessError) as exc:
            rec["errors"].append("power-off: " + str(exc))
        serial.close()
        rec["relay_status"] = serial.status
        raw = serial.raw()
        rec["serial_bytes"] = len(raw)
        rec["serial_sha256"] = hashlib.sha256(raw).hexdigest()
        rec["markers"] = [line for line in serial_lines(raw) if any(key in line for key in MARKER_KEYS)]
        rec["rejected"] = [key for key in REJECT if key.encode() in raw.lower()]
        log = run_dir / "vmware.log"
        if log.is_file():
            shutil.copyfile(log, evidence / f"boot{index}-vmware.log")
            rec["vmware"] = vmware_facts(log.read_text(encoding="utf-8", errors="replace"))
    return rec


def check_relay_input(serial, rec):
    out = serial.run("uname -a")
    printed = printed_lines(out, "uname -a")
    rec["commands"].append({"command": "uname -a", "printed": printed})
    rec["relay_input"] = any(line.startswith("Cellos cellos-kernel ") for line in printed)
    if not rec["relay_input"]:
        raise LaneError("typed 'uname -a' printed no 'Cellos cellos-kernel' line")


def write_nonce(nonce):
    def action(serial, rec):
        path = "/mnt/sd/" + PERSIST_FILE
        for command in (f"vwrite {path} {nonce}", f"vcat {path}"):
            out = serial.run(command)
            rec["commands"].append({"command": command, "printed": printed_lines(out, command)})
        rec["nonce_written"] = (not any("failed" in line for line in rec["commands"][-2]["printed"])
                                and nonce in rec["commands"][-1]["printed"])
        if not rec["nonce_written"]:
            raise LaneError("boot-1 write/read-back of the nonce failed")
    return action


def read_nonce(nonce):
    def action(serial, rec):
        command = "vcat /mnt/sd/" + PERSIST_FILE
        out = serial.run(command)
        printed = printed_lines(out, command)
        rec["commands"].append({"command": command, "printed": printed})
        rec["nonce_read"] = nonce in printed
        if not rec["nonce_read"]:
            raise LaneError("boot 2 did not read the boot-1 nonce back")
    return action


def wait_dhcp(serial, rec):
    """Give the guest DHCP_TIMEOUT from power-on for its lease; the boot's markers judge it."""
    rec["dhcp_lease"] = serial.wait_for(DHCP_LEASE, 0, time.monotonic() + DHCP_TIMEOUT)


def nic_errors(spec, rec):
    """NIC verdict of one boot: its own cell bound the VMware identity, registered, leased."""
    markers = rec.get("markers", [])
    errors = ["missing marker: " + text.strip() for text in (NIC_BIND[spec["nic"]], NIC_REGISTERED, DHCP_LEASE)
              if not any(text in line for line in markers)]
    idle = f"[{spec['nic']}] no "
    errors += ["NIC refused: " + line for line in markers
               if any(text in line for text in NIC_REFUSALS) or idle in line]
    return errors


def poweroff_errors(rec):
    if rec.get("guest_poweroff") and not rec.get("hard_stop"):
        return []
    return ["no guest S5 power-off: " + ("hard stop" if rec.get("hard_stop") else "VM still listed")]


def boot_expectations(spec, index, rec):
    """Errors for missing or forbidden boot markers of one boot record."""
    errors = []
    markers = rec.get("markers", [])
    def seen(text):
        return any(text in line for line in markers)
    if rec.get("prompt_seconds") is None:
        return ["no prompt"]
    if rec.get("rejected"):
        errors.append("rejected serial markers: " + ", ".join(rec["rejected"]))
    board = f"x86_64-{spec['profile']}"
    if not seen(f"[x86-gate] board={board} "):
        errors.append(f"board={board} not reported")
    if spec["cpus"] > 1 and not seen(f"[x86-smp] topology online={spec['cpus']} "):
        errors.append(f"topology online={spec['cpus']} not reported")
    if spec["storage"]:
        pin = "machine-pin: first boot" if index == 1 else "machine-pin: match"
        for text in ("[driver_cell] block driver registered: tid=", "[vfs] FAT32 /mnt/sd volume mounted", pin):
            if not seen(text):
                errors.append("missing marker: " + text)
        if seen("SAFE MODE"):
            errors.append("SAFE MODE with a provisioned disk")
    if spec["net"]:
        errors += nic_errors(spec, rec)
    if spec["poweroff"]:
        errors += poweroff_errors(rec)
    return errors


def lane_dimensions(spec, res):
    """Per-dimension verdicts (prompt, network, persistence, poweroff) of a judged cell."""
    boots = res["boots"]
    complete = len(boots) == spec["boots"]
    def every(check):
        return "PASS" if complete and all(check(rec) for rec in boots) else "FAIL"
    return {
        "prompt": every(lambda rec: rec.get("prompt_seconds") is not None),
        "network": every(lambda rec: rec.get("prompt_seconds") is not None and not nic_errors(spec, rec))
                   if spec["net"] else "N/A",
        "persistence": res["persistence"],
        "poweroff": every(lambda rec: not poweroff_errors(rec)) if spec["poweroff"] else "N/A",
    }


def decisive_failures(res):
    """Every failure reason of a cell, boot-scoped, in the order it arose."""
    lines = list(res["errors"])
    for rec in res["boots"]:
        lines += [f"boot {rec['boot']}: {text}" for text in rec.get("errors", []) + rec.get("expectation_errors", [])]
    return lines


def matrix_table(results):
    """One row per cell (status, dimensions, first decisive failure) and per-dimension totals."""
    rows = [{"cell": r["scenario"], "status": r["status"], **r.get("dimensions", {}),
             "decisive": (r.get("failures") or [None])[0]} for r in results]
    totals = {}
    for row in rows:
        for key in ("status", "prompt", "network", "persistence", "poweroff"):
            if key in row:
                totals.setdefault(key, {}).setdefault(row[key], 0)
                totals[key][row[key]] += 1
    return {"cells": rows, "totals": totals}


def lane_verdict(spec, res):
    """Fill relay_input / persistence / status from the boot records (pure)."""
    boots = res["boots"]
    for index, rec in enumerate(boots, 1):
        rec["expectation_errors"] = boot_expectations(spec, index, rec)
    first = boots[0] if boots else {}
    res["relay_input"] = "PASS" if first.get("relay_input") else ("FAIL" if first.get("prompt_seconds") is not None else "NOT-RUN")
    if spec["boots"] == 2:
        if res["relay_input"] != "PASS":
            res["persistence"] = "NOT-RUN"
            res["errors"].append("persistence NOT-RUN: relay input did not work")
        elif (len(boots) == 2 and first.get("nonce_written") and boots[1].get("nonce_read")
              and res.get("host_readback") == res.get("nonce")):
            res["persistence"] = "PASS"
        else:
            res["persistence"] = "FAIL"
    else:
        res["persistence"] = "N/A"
    if spec["probe"]:
        # A host-capability probe records what happened (e.g. a refused power-on);
        # it fails only when no VM start was attempted or cleanup failed.
        res["status"] = "OBSERVED" if boots and res["cleanup_confirmed"] else "FAIL"
        return res
    clean = (res["cleanup_confirmed"] and not res["errors"]
             and all(not rec["errors"] and not rec["expectation_errors"] and rec["stopped"] for rec in boots)
             and len(boots) == spec["boots"] and res["relay_input"] == "PASS"
             and res["persistence"] in ("PASS", "N/A"))
    res["status"] = "PASS" if clean else "FAIL"
    res["dimensions"] = lane_dimensions(spec, res)
    res["failures"] = [] if clean else decisive_failures(res)
    return res


def run_lane(vmrun, vdisk, iso, spec, run_id, evidence_root):
    spec = lane_spec(spec)
    run_dir = LANE_ROOT / f"{run_id}-{spec['name']}"
    evidence = evidence_root / spec["name"]
    evidence.mkdir(parents=True)
    nonce = f"S3LANE-{run_id}-{spec['name']}"
    res = {"scenario": spec["name"], "spec": spec, "vm_dir": VM.to_host_path(run_dir),
           "evidence_dir": str(evidence), "nonce": nonce if spec["boots"] == 2 else None,
           "status": "FAIL", "errors": [], "boots": [], "cleanup_confirmed": False}
    vmx = run_dir / "cellos.vmx"
    try:
        run_dir.mkdir(parents=True)
        shutil.copyfile(iso, run_dir / "cellos.iso")
        shutil.copyfile(RELAY, run_dir / RELAY.name)
        if spec["storage"]:
            make_lane_disk(vdisk, spec["storage"], run_dir, evidence)
        first = [check_relay_input] + ([write_nonce(nonce)] if spec["boots"] == 2 else [])
        second = [read_nonce(nonce)]
        if spec["net"]:
            first.append(wait_dhcp)
            second.append(wait_dhcp)
        res["boots"].append(lane_boot(vmrun, spec, run_dir, evidence, 1, first))
        if spec["boots"] == 2 and res["boots"][0].get("nonce_written"):
            res["boots"].append(lane_boot(vmrun, spec, run_dir, evidence, 2, second))
            if res["boots"][1]["stopped"]:
                res["host_readback"] = host_read(run_dir / "disk.vmdk", PERSIST_FILE)
    except (LaneError, OSError, subprocess.SubprocessError) as exc:
        res["errors"].append(str(exc))
    finally:
        try:
            if vmx.exists():
                if vm_listed(vmrun, vmx, evidence / "vmrun-list.log"):
                    VM.run_logged([str(vmrun), "-T", "ws", "stop", VM.to_host_path(vmx), "hard"],
                                  evidence / "cleanup-vmrun-stop.log", 60)
                code = VM.run_logged([str(vmrun), "-T", "ws", "deleteVM", VM.to_host_path(vmx)],
                                     evidence / "cleanup-vmrun-deletevm.log", 120)
                if code != 0:
                    res["errors"].append(f"vmrun deleteVM failed (exit {code})")
            if run_dir.exists():
                shutil.rmtree(run_dir)
            res["cleanup_confirmed"] = not run_dir.exists() and not vm_listed(
                vmrun, vmx, evidence / "vmrun-list.log")
        except (LaneError, OSError, subprocess.SubprocessError) as exc:
            res["errors"].append("cleanup: " + str(exc))
    lane_verdict(spec, res)
    (evidence / "result.json").write_text(json.dumps(res, indent=2) + "\n")
    return res


def lane_main(args):
    vmrun = VM.find_vmrun(None)
    if not VM.IS_WSL_WIN:
        raise LaneError("WSL-to-Windows VMware Workstation required")
    vdisk = vmrun.parent / "vmware-vdiskmanager.exe"
    isos = {"vm": Path(args.iso), "pc": Path(args.pc_iso)}
    if args.command == "lane":
        specs = [{"name": args.name, "nic": args.nic, "storage": args.storage, "cpus": args.cpus,
                  "firmware": args.firmware, "viommu": args.viommu, "boots": args.boots,
                  "profile": args.profile}]
    elif args.command == "matrix":
        specs = select_scenarios(matrix_scenarios(), args.only)
    else:
        specs = [dict(spec) for spec in V0_SCENARIOS]
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:6]
    evidence_root = EVIDENCE_ROOT / run_id
    evidence_root.mkdir(parents=True)
    LANE_ROOT.mkdir(parents=True, exist_ok=True)
    print(f"LANE_EVIDENCE {evidence_root}", flush=True)
    host = {"vmrun": str(vmrun)}
    for key, command in (("vmrun_version", [str(vmrun)]), ("vdiskmanager", [str(vdisk)]),
                         ("relay_python", ["py.exe", "-3", "--version"])):
        out = subprocess.run(command, capture_output=True, text=True, cwd=str(LANE_ROOT))
        match = re.search(r"(vmrun version [^\r\n]*|build \d+|Python [^\r\n]*)", out.stdout + out.stderr)
        host[key] = match.group(0) if match else None
    used = {}
    for spec in specs:
        spec.setdefault("memory_mib", args.memory_mib)
        spec.setdefault("window", args.window)
        profile = spec.get("profile", "vm")
        iso = isos[profile].resolve(strict=True)
        used[profile] = {"iso": str(iso), "sha256": sha256(iso)}

    def one(spec):
        print(f"lane {spec['name']} ...", flush=True)
        res = run_lane(vmrun, vdisk, isos[spec.get("profile", "vm")].resolve(strict=True), spec,
                       run_id, evidence_root)
        dims = " ".join(f"{k}={v}" for k, v in res.get("dimensions", {}).items())
        print(f"  {spec['name']}: {res['status']} relay_input={res['relay_input']} {dims} "
              f"cleanup={res['cleanup_confirmed']} failures={res.get('failures', res['errors'])}", flush=True)
        return res

    # Cells are independent VMs (own directory, pipe and disk), so they may run side by side.
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        results = list(pool.map(one, specs))
    summary = {
        "format": "cellos-vmware-lane-v1", "run_id": run_id, "command": args.command,
        "ceiling": "VMware local: Workstation on a Windows host with Hyper-V, driven from WSL2; "
                   "not CI, ESXi or physical hardware",
        "host": host, "isos": used, "lane_root": VM.to_host_path(LANE_ROOT),
        "hw_version_requested": VM.HW_VERSION, "jobs": args.jobs,
        "matrix": matrix_table(results), "results": results,
    }
    out = Path(args.json) if args.json else evidence_root / "summary.json"
    out.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"SUMMARY {out}")
    return 0 if all(r["status"] in ("PASS", "OBSERVED") for r in results) else 1


def fault_main():
    vmrun = VM.find_vmrun(None)
    if not VM.IS_WSL_WIN:
        raise RuntimeError("WSL-to-Windows VMware Workstation required")

    iso_src = ROOT / "build/vicell-x86.iso"
    if not iso_src.is_file():
        raise RuntimeError("Missing build/vicell-x86.iso")

    raw_disk_src = ROOT / "build/durable-charge-vmware-lab/native-profile-probe.img"
    if not raw_disk_src.is_file():
        raise RuntimeError("Missing raw disk source")

    parent = Path("/mnt/c/Temp/cellos-vmware")
    ws = Path(tempfile.mkdtemp(prefix="native-vmware-matrix-", dir=parent))
    print(f"NATIVE_MATRIX_WORKSPACE {ws}", flush=True)

    iso = ws / "cellos.iso"
    shutil.copyfile(iso_src, iso)

    # Convert raw disk to monolithicSparse IDE/SATA VMDK
    disk_vmdk = ws / "native-sata.vmdk"
    subprocess.run(["qemu-img", "convert", "-f", "raw", "-O", "vmdk", "-o", "subformat=monolithicSparse,adapter_type=ide", str(raw_disk_src), str(disk_vmdk)], check=True)

    results = []

    # Scenario 1: Basic mount
    print("Running Scenario 1: native-ahci-mount-basic...", flush=True)
    res1 = run_native_boot(vmrun, ws, "native-ahci-mount-basic", iso, disk_vmdk)
    print(f"  Result 1: {res1['status']} (errors={res1['errors']})", flush=True)
    results.append(res1)

    # Scenario 2: Reboot with same disk (persistence of volume headers)
    print("Running Scenario 2: native-ahci-mount-reboot...", flush=True)
    res2 = run_native_boot(vmrun, ws, "native-ahci-mount-reboot", iso, disk_vmdk)
    print(f"  Result 2: {res2['status']} (errors={res2['errors']})", flush=True)
    results.append(res2)

    # Scenario 3: No-disk degradation
    print("Running Scenario 3: native-ahci-no-disk...", flush=True)
    res3 = run_native_boot(vmrun, ws, "native-ahci-no-disk", iso, None)
    print(f"  Result 3: {res3['status']} (errors={res3['errors']})", flush=True)
    results.append(res3)

    # Scenario 4: Corrupt disk refusal
    print("Running Scenario 4: native-ahci-corrupt-refusal...", flush=True)
    corrupt_raw = ws / "corrupt.raw"
    corrupt_p5_superblocks(raw_disk_src, corrupt_raw)
    corrupt_vmdk = ws / "corrupt-sata.vmdk"
    subprocess.run(["qemu-img", "convert", "-f", "raw", "-O", "vmdk", "-o", "subformat=monolithicSparse,adapter_type=ide", str(corrupt_raw), str(corrupt_vmdk)], check=True)
    res4 = run_native_boot(vmrun, ws, "native-ahci-corrupt-refusal", iso, corrupt_vmdk)
    print(f"  Result 4: {res4['status']} (errors={res4['errors']})", flush=True)
    results.append(res4)

    summary = {
        "format": "native-cellos-vmware-fault-matrix-v1",
        "workspace": str(ws),
        "scope": "Storage boot/mount/refusal only; not journal, SMP, VMX or distributed qualification",
        "iso_sha256": sha256(iso),
        "results": results,
        "all_passed": all(r["status"] == "PASS" for r in results),
    }

    summary_file = ws / "matrix-summary.json"
    summary_file.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"SUMMARY {summary_file}")
    if not summary["all_passed"]:
        sys.exit(1)

def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command")
    commands.add_parser("fault", help="native AHCI storage/fault matrix (default)")
    for name in ("lane", "v0", "matrix"):
        sub = commands.add_parser(name, help={"lane": "one lane cell", "v0": "S3 V0 tooling smoke",
                                              "matrix": "phase-09 VMware matrix sweep"}[name])
        sub.add_argument("--iso", default=str(ROOT / "build/vicell-x86-vm.iso"),
                         help="x86_64-vm ISO (make-iso-ci.sh with limine-vm.conf)")
        sub.add_argument("--pc-iso", default=str(ROOT / "build/vicell-x86-pc.iso"),
                         help="profile-pc ISO (make-iso-ci.sh with limine-pc.conf)")
        sub.add_argument("--memory-mib", type=int, default=1024)
        sub.add_argument("--window", type=float, default=120.0, help="seconds from vmrun start to the prompt")
        sub.add_argument("--json", help="summary JSON path (default: <evidence>/summary.json)")
        sub.add_argument("--jobs", type=int, default=1, choices=range(1, 7), metavar="1-6",
                         help="cells run side by side (default 1)")
        if name == "matrix":
            sub.add_argument("--only", action="append", default=[], metavar="GLOB",
                             help="run only the cells whose name matches (repeatable)")
        if name == "lane":
            sub.add_argument("--name", default="lane")
            sub.add_argument("--nic", choices=VM.NIC_MODELS)
            sub.add_argument("--storage", choices=VM.STORAGE_MODELS)
            sub.add_argument("--cpus", type=int, choices=(1, 2, 4), default=2)
            sub.add_argument("--firmware", choices=VM.FIRMWARES, default="bios")
            sub.add_argument("--viommu", action="store_true", help='set "Virtualize IOMMU" (vvtd.enable)')
            sub.add_argument("--boots", type=int, choices=(1, 2), default=1)
            sub.add_argument("--profile", choices=("vm", "pc"), default="vm")
    args = parser.parse_args()
    if args.command in (None, "fault"):
        fault_main()
        return 0
    try:
        return lane_main(args)
    except (LaneError, OSError, subprocess.SubprocessError) as exc:
        print("FAIL: " + str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
