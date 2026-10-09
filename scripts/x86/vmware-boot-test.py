#!/usr/bin/env python3
"""Boot the production x86 ISO in native Linux/Windows VMware Workstation Pro.

Requires Python 3.8+ and a working Workstation Pro installation. No VMware Tools
are needed. This is BIOS boot compatibility evidence, not hardware/driver or SMP
qualification. Two vCPUs only expose that configuration to the existing kernel.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone


IS_WSL_WIN = False

class BootFailure(Exception):
    pass


def to_host_path(path):
    if IS_WSL_WIN:
        return subprocess.check_output(["wslpath", "-m", str(path)], text=True).strip()
    return str(path)

def positive_seconds(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be a finite positive number")
    return number


def vmx_string(value):
    # Use forward slashes for native Windows paths. Reject VMX quoting/escape
    # characters instead of interpolating ambiguous strings into a config file.
    text = value.as_posix() if isinstance(value, Path) else str(value)
    if any(ord(char) < 32 or ord(char) == 127 or char in '\\"|' for char in text):
        raise BootFailure("path/value contains unsupported VMX characters: " + repr(text))
    return '"' + text + '"'


def find_vmrun(override):
    global IS_WSL_WIN
    if override:
        candidate = Path(override).expanduser().resolve()
        if not candidate.is_file() or not os.access(candidate, os.X_OK):
            raise BootFailure("--vmrun must name an executable file: " + str(candidate))
        if os.name != "nt" and candidate.suffix.lower() == ".exe":
            if not shutil.which("wslpath"):
                raise BootFailure("Windows vmrun.exe requires native Windows Python or WSL with wslpath")
            IS_WSL_WIN = True
        return candidate
    candidates = []
    name = "vmrun.exe" if os.name == "nt" else "vmrun"
    located = shutil.which(name)
    if located:
        candidates.append(Path(located))
    if os.name == "nt":
        roots = [os.environ.get("ProgramFiles", r"C:\Program Files"),
                 os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"),
                 os.environ.get("ProgramW6432", r"C:\Program Files")]
        candidates.extend(Path(root) / "VMware" / "VMware Workstation" / name
                          for root in roots)
    else:
        candidates.extend(Path(path) for path in
                          ("/usr/bin/vmrun", "/usr/local/bin/vmrun",
                           "/usr/lib/vmware/bin/vmrun", "/usr/lib/vmware-vix/vmrun",
                           "/mnt/c/Program Files (x86)/VMware/VMware Workstation/vmrun.exe",
                           "/mnt/c/Program Files/VMware/VMware Workstation/vmrun.exe"))
    for candidate in candidates:
        if candidate.is_file() and os.access(candidate, os.X_OK):
            if os.name != "nt" and candidate.suffix.lower() == ".exe":
                if shutil.which("wslpath"):
                    IS_WSL_WIN = True
                else:
                    continue
            return candidate.resolve()
    searched = ", ".join(str(path) for path in candidates)
    raise BootFailure("VMware Workstation Pro vmrun is not installed/discoverable; "
                      "install Workstation Pro on native Linux or Windows, or supply "
                      "--vmrun PATH. Searched PATH and " + searched)


def make_vmx(iso, serial, cpus, memory):
    entries = {
        ".encoding": "UTF-8", "config.version": "8",
        "virtualHW.version": "10", "displayName": "Cellos x86 boot compatibility",
        "guestOS": "other-64", "firmware": "bios", "memsize": str(memory),
        "numvcpus": str(cpus), "cpuid.coresPerSocket": str(cpus),
        "bios.bootOrder": "cdrom", "msg.autoAnswer": "TRUE",
        # Cellos requires an ACPI-described HPET for its timer/sleep gate.
        "hpet0.present": "TRUE",
        "ide1:0.present": "TRUE", "ide1:0.deviceType": "cdrom-image",
        "ide1:0.fileName": iso, "ide1:0.startConnected": "TRUE",
        "serial0.present": "TRUE", "serial0.fileType": "file",
        "serial0.fileName": serial, "serial0.startConnected": "TRUE",
        "serial0.yieldOnMsrRead": "TRUE", "serial0.tryNoRxLoss": "FALSE",
        "ethernet0.present": "FALSE", "floppy0.present": "FALSE",
        "sound.present": "FALSE", "usb.present": "FALSE",
        "ehci.present": "FALSE", "usb_xhci.present": "FALSE",
        "vmci0.present": "FALSE", "sata0.present": "FALSE",
        "scsi0.present": "FALSE", "mks.enable3d": "FALSE",
        "isolation.tools.hgfs.disable": "TRUE",
    }
    return "".join(key + " = " + vmx_string(value) + "\n"
                   for key, value in entries.items())


def run_logged(command, logfile, timeout):
    with logfile.open("wb") as stream:
        process = subprocess.Popen(command, stdout=stream, stderr=subprocess.STDOUT)
        try:
            return process.wait(timeout=timeout)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()


def read_serial(path):
    try:
        return path.read_bytes()
    except FileNotFoundError:
        return b""


def reject_serial(data):
    lowered = data.lower()
    if b"kernel panic" in lowered or b"[fault] cell" in lowered:
        raise BootFailure("kernel panic / cell fault detected in serial.log")
    if b"tier 2 admission: enabled" in lowered:
        raise BootFailure("production image enabled Tier 2 admission")


def stopped_vm(vmrun, vmx, workspace, timeout):
    target = to_host_path(vmx)
    code = run_logged([str(vmrun), "-T", "ws", "stop", target, "hard"],
                      workspace / "vmrun-stop.log", timeout)
    if code != 0:
        raise BootFailure("hard stop failed (exit " + str(code) + "); VM may still be running: " + str(vmx))
    log = workspace / "vmrun-list-after-stop.log"
    code = run_logged([str(vmrun), "-T", "ws", "list"], log, timeout)
    if code != 0:
        raise BootFailure("cannot confirm own VM stopped; vmrun list failed")
    lines = log.read_text(encoding="utf-8", errors="replace").splitlines()
    if not lines or not re.fullmatch(r"Total running VMs: \d+", lines[0].strip()):
        raise BootFailure("cannot confirm own VM stopped; unrecognized vmrun list output")
    target = to_host_path(vmx)
    own_norm = os.path.normcase(target)
    if any(os.path.normcase(line.strip()) == own_norm or os.path.normcase(line.strip()).replace("\\", "/") == own_norm for line in lines[1:]):
        raise BootFailure("own VM remains running after hard stop: " + str(vmx))


def interrupt(signum, frame):
    raise KeyboardInterrupt


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="Examples:\n"
               "  python3 scripts/x86/vmware-boot-test.py --window 90\n"
               "  py -3 scripts/x86/vmware-boot-test.py build/vicell-x86.iso --cpus 2\n"
               "Failures retain evidence and exit nonzero; missing VMware is not a skip.")
    parser.add_argument("iso", nargs="?", default="build/vicell-x86.iso",
                        help="production Limine ISO (default: build/vicell-x86.iso)")
    parser.add_argument("--vmrun", help="explicit native vmrun executable path")
    parser.add_argument("--cpus", type=int, choices=(1, 2), default=1,
                        help="vCPU exposure, not SMP qualification (default: 1)")
    parser.add_argument("--memory-mib", type=int, choices=(256, 512), default=256,
                        help="guest memory in MiB (default: 256)")
    parser.add_argument("--window", type=positive_seconds, default=90.0,
                        help="observation seconds including VM startup (default: 90)")
    parser.add_argument("--command-timeout", type=positive_seconds, default=30.0,
                        help="version/cleanup command timeout seconds (default: 30)")
    parser.add_argument("--evidence-root", default="build/x86-backend-evidence/vmware",
                        help="parent of unique retained run directories (default: build/x86-backend-evidence/vmware)")
    args = parser.parse_args()

    try:
        if os.name not in ("nt", "posix") or (os.name == "posix" and platform.system() != "Linux"):
            raise BootFailure("only native Linux and Windows Workstation hosts are supported")
        vmrun = find_vmrun(args.vmrun)
        if IS_WSL_WIN and args.evidence_root == "build/x86-backend-evidence/vmware":
            root = Path("/mnt/c/Temp/cellos-vmware")
        else:
            root = Path(args.evidence_root).expanduser().resolve()
        vmx_string(root)
        root.mkdir(parents=True, exist_ok=True)
        prefix = datetime.now(timezone.utc).strftime("run-%Y%m%dT%H%M%SZ-")
        workspace = Path(tempfile.mkdtemp(prefix=prefix, dir=str(root)))
    except (BootFailure, OSError) as exc:
        print("FAIL: " + str(exc), file=sys.stderr)
        return 1

    print("Evidence: " + str(workspace), flush=True)
    vmx = workspace / "cellos.vmx"
    serial = workspace / "serial.log"
    result = {
        "backend": "vmware-workstation", "status": "FAIL",
        "host": platform.platform(), "started_utc": datetime.now(timezone.utc).isoformat(),
        "cpus": args.cpus, "memory_mib": args.memory_mib, "firmware": "bios",
        "window_seconds": args.window, "evidence_dir": str(workspace),
        "boot_to_prompt_seconds": None,
        "metric_scope": "host monotonic time from immediately before vmrun start to first serial prompt observation; includes VM startup; polling resolution 0.1 seconds",
        "qualification": "boot compatibility only; not physical hardware, driver, interrupt/I/O or SMP qualification",
        "errors": [], "cleanup_confirmed": False,
    }
    start_process = None
    start_stream = None
    startup_attempted = False
    start_time = None
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, interrupt)
    if hasattr(signal, "SIGBREAK"):
        signal.signal(signal.SIGBREAK, interrupt)

    try:
        result["vmrun"] = str(vmrun)
        iso = Path(args.iso).expanduser().resolve(strict=True)
        vmx_string(iso)
        if not iso.is_file():
            raise BootFailure("ISO must be a regular file: " + str(iso))
        if IS_WSL_WIN and not str(iso).startswith("/mnt/c/"):
            local_iso = workspace / "cellos.iso"
            shutil.copy(iso, local_iso)
            iso = local_iso
        digest = hashlib.sha256()
        with iso.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        result["iso"] = str(iso)
        result["iso_sha256"] = digest.hexdigest()
        (workspace / "iso.sha256").write_text(digest.hexdigest() + "  " + str(iso) + "\n", encoding="utf-8")
        text = make_vmx(to_host_path(iso), to_host_path(serial), args.cpus, args.memory_mib)
        vmx.write_text(text, encoding="utf-8")
        # VMware may rewrite its VMX; retain the exact requested configuration.
        (workspace / "requested.vmx").write_text(text, encoding="utf-8")
        version_log = workspace / "vmrun-version.log"
        result["vmrun_version_exit"] = run_logged([str(vmrun)], version_log, args.command_timeout)
        version_text = version_log.read_text(encoding="utf-8", errors="replace")
        match = re.search(r"(?im)^.*vmrun version[^\r\n]*", version_text)
        if not match:
            raise BootFailure("vmrun did not report its version; see vmrun-version.log")
        result["vmrun_version"] = match.group(0).strip()

        command = [str(vmrun), "-T", "ws", "start", to_host_path(vmx), "nogui"]
        result["start_command"] = command
        start_stream = (workspace / "vmrun-start.log").open("wb")
        start_time = time.monotonic()
        startup_attempted = True
        start_process = subprocess.Popen(command, stdout=start_stream, stderr=subprocess.STDOUT)
        deadline = start_time + args.window
        # Observe while vmrun start is pending too, so startup latency is counted
        # without delaying the first prompt observation until vmrun returns.
        while True:
            data = read_serial(serial)
            observed = time.monotonic()
            reject_serial(data)
            if observed <= deadline and b"Cellos >" in data and result["boot_to_prompt_seconds"] is None:
                result["boot_to_prompt_seconds"] = observed - start_time
            code = start_process.poll()
            if code is not None and code != 0:
                raise BootFailure("vmrun start failed (exit " + str(code) + "); see vmrun-start.log")
            remaining = deadline - observed
            if remaining <= 0:
                break
            time.sleep(min(0.1, remaining))
        if start_process.poll() is None:
            raise BootFailure("vmrun start did not finish within the observation window")
        if result["boot_to_prompt_seconds"] is None:
            raise BootFailure("'Cellos >' not observed within the window including startup")
        if b"Tier 2 admission: DISABLED" not in data:
            raise BootFailure("production image did not report Tier 2 admission: DISABLED")
    except KeyboardInterrupt:
        result["errors"].append("interrupted")
    except (BootFailure, OSError, subprocess.SubprocessError) as exc:
        result["errors"].append(str(exc))
    finally:
        # Do not let a second terminal signal interrupt the hard-stop attempt.
        for signum in (signal.SIGINT, signal.SIGTERM):
            signal.signal(signum, signal.SIG_IGN)
        if hasattr(signal, "SIGBREAK"):
            signal.signal(signal.SIGBREAK, signal.SIG_IGN)
        if start_process is not None and start_process.poll() is None:
            try:
                start_process.kill()
                start_process.wait()
            except OSError as exc:
                result["errors"].append("terminating pending vmrun start: " + str(exc))
        if start_stream is not None:
            start_stream.close()
        if startup_attempted:
            try:
                stopped_vm(vmrun, vmx, workspace, args.command_timeout)
                result["cleanup_confirmed"] = True
            except (BootFailure, OSError, subprocess.SubprocessError) as exc:
                result["errors"].append("cleanup: " + str(exc))
        try:
            backend_log = workspace / "vmware.log"
            if backend_log.exists():
                backend_text = backend_log.read_text(encoding="utf-8", errors="replace")
                match = re.search(r"Log for VMware Workstation[^\r\n]*", backend_text)
                if match:
                    result["backend_version"] = match.group(0)
            if startup_attempted and "backend_version" not in result:
                result["errors"].append("Workstation backend version missing from vmware.log")
            # Recheck after power-off, including output flushed during cleanup.
            reject_serial(read_serial(serial))
        except (BootFailure, OSError) as exc:
            if str(exc) not in result["errors"]:
                result["errors"].append(str(exc))
        if start_time is not None:
            result["startup_observation_and_cleanup_seconds"] = time.monotonic() - start_time
        if not result["errors"] and result["cleanup_confirmed"]:
            result["status"] = "PASS"
        try:
            (workspace / "result.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        except OSError as exc:
            result["status"] = "FAIL"
            result["errors"].append("cannot save result.json: " + str(exc))

    if result["status"] != "PASS":
        for error in result["errors"]:
            print("FAIL: " + error, file=sys.stderr)
        return 1
    print("PASS: production x86_64 VMware BIOS boot; boot-to-prompt "
          + format(result["boot_to_prompt_seconds"], ".3f")
          + "s (includes VM startup); own VM hard-stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
