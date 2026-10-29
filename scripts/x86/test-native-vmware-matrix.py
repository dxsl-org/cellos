#!/usr/bin/env python3
"""Execute the native Cellos x86 storage and fault matrix on VMware Workstation."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

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
    entries = {
        ".encoding": "UTF-8", "config.version": "8", "virtualHW.version": "14",
        "displayName": "Cellos native storage qualification", "guestOS": "other-64",
        "firmware": "bios", "memsize": "256", "numvcpus": "1", "cpuid.coresPerSocket": "1",
        "bios.bootOrder": "cdrom", "msg.autoAnswer": "TRUE", "hpet0.present": "TRUE",
        "ide1:0.present": "TRUE", "ide1:0.deviceType": "cdrom-image",
        "ide1:0.fileName": VM.to_host_path(iso), "ide1:0.startConnected": "TRUE",
        "serial0.present": "TRUE", "serial0.fileType": "file",
        "serial0.fileName": VM.to_host_path(serial), "serial0.startConnected": "TRUE",
        "serial0.yieldOnMsrRead": "TRUE", "ethernet0.present": "FALSE", "floppy0.present": "FALSE",
        "sound.present": "FALSE", "usb.present": "FALSE", "ehci.present": "FALSE",
        "usb_xhci.present": "FALSE", "vmci0.present": "FALSE", "mks.enable3d": "FALSE",
        "isolation.tools.hgfs.disable": "TRUE",
    }
    if disk:
        entries["sata0.present"] = "TRUE"
        entries["sata0:0.present"] = "TRUE"
        entries["sata0:0.fileName"] = VM.to_host_path(disk)
    else:
        entries["sata0.present"] = "FALSE"
    return "".join(k + " = " + VM.vmx_string(v) + "\n" for k, v in entries.items())

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

def main():
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

if __name__ == "__main__":
    main()
