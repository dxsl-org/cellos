"""Runner verdict and VMX-generation regressions; synthetic VM outcomes are not native runtime evidence."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "native_matrix", Path(__file__).with_name("test-native-vmware-matrix.py"))
MATRIX = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MATRIX)

AHCI = b"[driver_cell] ahci storage driver ready (port=0 sectors=1062144 identify ok)\n"
MOUNT = b"[vfs] CellosFS Native mounted at existing volume\n"
REFUSE = "[vfs] WARNING: CellosFS open failed — volume unavailable\n".encode()
SHELL = b"Cellos >\n"


class NativeMatrixVerdictTests(unittest.TestCase):
    def run_scenario(self, name, serial, start=0, cleanup=None, digests=None, serial_reads=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            disk = root / "disk.vmdk"
            disk.write_bytes(b"container fixture")
            with patch.object(MATRIX.VM, "to_host_path", side_effect=str), \
                 patch.object(MATRIX.VM, "run_logged", return_value=start), \
                 patch.object(MATRIX.VM, "stopped_vm", side_effect=cleanup), \
                 patch.object(MATRIX.VM, "read_serial", return_value=serial, side_effect=serial_reads), \
                 patch.object(MATRIX.time, "monotonic", side_effect=[100, 100, 100, 102]), \
                 patch.object(MATRIX.time, "sleep"), \
                 patch.object(MATRIX, "raw_disk_sha256", side_effect=digests or ["same", "same"]):
                return MATRIX.run_native_boot("vmrun", root, name, root / "image.iso",
                                              disk if name != "native-ahci-no-disk" else None,
                                              window=1)

    def test_start_failure_cannot_pass_even_with_success_markers(self):
        for serial in (b"", AHCI + MOUNT + REFUSE + SHELL):
            with self.subTest(serial=serial):
                result = self.run_scenario("native-ahci-corrupt-refusal", serial, start=1)
                self.assertEqual(result["status"], "FAIL")
                self.assertTrue(any("start failed" in e for e in result["errors"]))

    def test_cleanup_failure_cannot_pass_a_healthy_boot(self):
        result = self.run_scenario("native-ahci-mount-basic", AHCI + MOUNT * 2 + SHELL,
                                   cleanup=RuntimeError("still running"))
        self.assertEqual(result["status"], "FAIL")
        self.assertFalse(result["cleanup"])

    def test_refusal_requires_boot_refusal_and_exact_mount_count(self):
        for serial in (b"", SHELL, AHCI + MOUNT * 2 + SHELL,
                       AHCI + REFUSE + SHELL, AHCI + MOUNT + REFUSE,
                       AHCI + MOUNT + REFUSE + SHELL + b"panic",
                       AHCI + MOUNT + REFUSE + SHELL + b"[vfs] CellosFS Native formatted and mounted volume"):
            with self.subTest(serial=serial):
                self.assertEqual(self.run_scenario("native-ahci-corrupt-refusal", serial)["status"], "FAIL")

    def test_refusal_rejects_disk_mutation_and_unreadable_disk(self):
        for digests in (["before", "after"], ["before", RuntimeError("read failed")]):
            with self.subTest(digests=digests):
                result = self.run_scenario("native-ahci-corrupt-refusal",
                                           AHCI + MOUNT + REFUSE + SHELL, digests=digests)
                self.assertEqual(result["status"], "FAIL")

    def test_complete_scenarios_pass_at_storage_only_ceiling(self):
        cases = (("native-ahci-mount-basic", AHCI + MOUNT * 2 + SHELL),
                 ("native-ahci-mount-reboot", AHCI + MOUNT * 2 + SHELL),
                 ("native-ahci-no-disk", SHELL),
                 ("native-ahci-corrupt-refusal", AHCI + MOUNT + REFUSE + SHELL))
        for name, serial in cases:
            with self.subTest(name=name):
                result = self.run_scenario(name, serial)
                self.assertEqual(result["status"], "PASS")
                self.assertEqual(result["errors"], [])

    def test_mount_without_shell_and_no_disk_with_mount_fail(self):
        cases = (("native-ahci-mount-basic", AHCI + MOUNT * 2),
                 ("native-ahci-no-disk", MOUNT + SHELL),
                 ("unknown-scenario", SHELL))
        for name, serial in cases:
            with self.subTest(name=name):
                self.assertEqual(self.run_scenario(name, serial)["status"], "FAIL")

    def test_post_deadline_shell_cannot_turn_timeout_into_pass(self):
        result = self.run_scenario("native-ahci-mount-basic", b"",
                                   serial_reads=[b"", AHCI + MOUNT * 2 + SHELL])
        self.assertEqual(result["status"], "FAIL")
        self.assertTrue(any("no terminal boot marker" in e for e in result["errors"]))

    def test_lookalike_refusal_does_not_qualify_corrupt_volume(self):
        serial = AHCI + MOUNT + b"[vfs] CellosFS open failed but continuing\n" + SHELL
        self.assertEqual(self.run_scenario("native-ahci-corrupt-refusal", serial)["status"], "FAIL")

    def test_shell_mount_and_ahci_lookalikes_are_not_success_markers(self):
        cases = (b"not " + AHCI + MOUNT + REFUSE + SHELL,
                 AHCI + b"not " + MOUNT + REFUSE + SHELL,
                 AHCI + MOUNT + REFUSE + b"not " + SHELL,
                 AHCI.replace(b"port=0", b"port=32") + MOUNT + REFUSE + SHELL,
                 AHCI.replace(b"sectors=1062144", b"sectors=0") + MOUNT + REFUSE + SHELL,
                 AHCI.replace(b"sectors=1062144", b"sectors=18446744073709551616") + MOUNT + REFUSE + SHELL)
        for serial in cases:
            with self.subTest(serial=serial):
                self.assertEqual(self.run_scenario("native-ahci-corrupt-refusal", serial)["status"], "FAIL")


def vmx_keys(text):
    pairs = [line.split(" = ", 1) for line in text.splitlines()]
    keys = [key for key, _ in pairs]
    assert len(keys) == len(set(keys)), "duplicate VMX key"
    return {key: value.strip('"') for key, value in pairs}


PIPE = r"\\.\pipe\cellos-run-b1"


class LaneVmxGenerationTests(unittest.TestCase):
    def test_pipe_serial_is_a_server_pipe_without_rx_loss(self):
        keys = vmx_keys(MATRIX.VM.make_vmx("C:/t/c.iso", PIPE, 2, 1024, pipe=True))
        self.assertEqual(keys["serial0.fileType"], "pipe")
        self.assertEqual(keys["serial0.pipe.endPoint"], "server")
        self.assertEqual(keys["serial0.fileName"], PIPE)
        self.assertEqual(keys["serial0.tryNoRxLoss"], "TRUE")

    def test_file_serial_keeps_the_file_shape(self):
        keys = vmx_keys(MATRIX.VM.make_vmx("C:/t/c.iso", "C:/t/serial.log", 1, 256))
        self.assertEqual((keys["serial0.fileType"], keys["serial0.fileName"]), ("file", "C:/t/serial.log"))
        self.assertNotIn("serial0.pipe.endPoint", keys)

    def test_only_lane_pipe_names_reach_the_vmx(self):
        for name in (r"\\.\pipe\other", r"\\.\pipe\cellos-a\b", '\\\\.\\pipe\\cellos-a"b',
                     r"\\.\pipe\cellos-", "C:/t/serial.log", r"\\.\pipe\cellos-a|22"):
            with self.subTest(name=name), self.assertRaises(MATRIX.VM.BootFailure):
                MATRIX.VM.make_vmx("C:/t/c.iso", name, 2, 1024, pipe=True)

    def test_every_matrix_cell_generates_its_devices(self):
        controllers = {"sata": "sata0", "nvme": "nvme0", "pvscsi": "scsi0"}
        for nic in MATRIX.VM.NIC_MODELS:
            for storage in MATRIX.VM.STORAGE_MODELS:
                for cpus in (2, 4):
                    for firmware in MATRIX.VM.FIRMWARES:
                        with self.subTest(nic=nic, storage=storage, cpus=cpus, firmware=firmware):
                            keys = vmx_keys(MATRIX.VM.make_vmx(
                                "C:/t/c.iso", PIPE, cpus, 1024, firmware=firmware, nic=nic,
                                storage=storage, disk="C:/t/disk.vmdk", pipe=True))
                            self.assertEqual(keys["virtualHW.version"], MATRIX.VM.HW_VERSION)
                            self.assertEqual((keys["numvcpus"], keys["firmware"]), (str(cpus), firmware))
                            self.assertEqual(keys.get("uefi.secureBoot.enabled"),
                                             "FALSE" if firmware == "efi" else None)
                            self.assertEqual((keys["ethernet0.present"], keys["ethernet0.virtualDev"],
                                              keys["ethernet0.connectionType"]), ("TRUE", nic, "nat"))
                            for kind, controller in controllers.items():
                                expected = "TRUE" if kind == storage else "FALSE"
                                self.assertEqual(keys[controller + ".present"], expected)
                            own = controllers[storage]
                            self.assertEqual(keys[own + ":0.fileName"], "C:/t/disk.vmdk")
                            self.assertEqual(keys.get("scsi0.virtualDev"),
                                             "pvscsi" if storage == "pvscsi" else None)
                            self.assertNotIn("vvtd.enable", keys)

    def test_pcie_root_ports_always_legacy_bridge_on_request(self):
        keys = vmx_keys(MATRIX.VM.make_vmx("C:/t/c.iso", PIPE, 2, 1024, pipe=True))
        for port in range(4, 8):
            self.assertEqual((keys[f"pciBridge{port}.present"], keys[f"pciBridge{port}.virtualDev"],
                              keys[f"pciBridge{port}.functions"]), ("TRUE", "pcieRootPort", "8"))
        self.assertNotIn("pciBridge0.present", keys)
        keys = vmx_keys(MATRIX.VM.make_vmx("C:/t/c.iso", PIPE, 2, 1024, pipe=True, legacy_bridge=True))
        self.assertEqual(keys["pciBridge0.present"], "TRUE")

    def test_viommu_and_no_device_defaults(self):
        keys = vmx_keys(MATRIX.VM.make_vmx("C:/t/c.iso", PIPE, 2, 1024, firmware="efi",
                                           pipe=True, viommu=True))
        self.assertEqual(keys["vvtd.enable"], "TRUE")
        self.assertEqual(keys["ethernet0.present"], "FALSE")
        self.assertNotIn("ethernet0.virtualDev", keys)

    def test_invalid_device_choices_are_refused(self):
        cases = ({"firmware": "csm"}, {"nic": "rtl8139"}, {"storage": "sata"},
                 {"storage": "ide", "disk": "C:/t/d.vmdk"}, {"disk": "C:/t/d.vmdk"})
        for kwargs in cases:
            with self.subTest(kwargs=kwargs), self.assertRaises(MATRIX.VM.BootFailure):
                MATRIX.VM.make_vmx("C:/t/c.iso", PIPE, 2, 1024, pipe=True, **kwargs)

    def test_native_fault_vmx_uses_the_pinned_generator(self):
        with patch.object(MATRIX.VM, "to_host_path", side_effect=str):
            keys = vmx_keys(MATRIX.make_native_vmx(Path("/t/c.iso"), Path("/t/s.log"), Path("/t/d.vmdk")))
        self.assertEqual(keys["virtualHW.version"], MATRIX.VM.HW_VERSION)
        self.assertEqual((keys["sata0.present"], keys["sata0:0.fileName"]), ("TRUE", "/t/d.vmdk"))
        self.assertEqual(keys["serial0.fileType"], "file")


NONCE = "S3LANE-run-cell"


def lane_boot_record(index, cpus=2, **override):
    pin = "Init: machine pin: first boot — recorded 6b80bc658c66" if index == 1 else "Init: machine pin: match 6b80bc658c66"
    rec = {"boot": index, "errors": [], "prompt_seconds": 4.0, "stopped": True, "rejected": [],
           "markers": ["[ INFO] [x86-gate] board=x86_64-vm soc-profile=x86_64-vm source=cmdline dma_isolation=Optional",
                       f"[ INFO] [x86-smp] topology online={cpus} processor_ids=0;1 qualified=yes",
                       "[ WARN] [driver_cell] block driver registered: tid=5 device=pci-00-10-0 route=0",
                       "[vfs] FAT32 /mnt/sd volume mounted", pin]}
    rec.update({"relay_input": True, "nonce_written": True} if index == 1 else {"nonce_read": True})
    rec.update(override)
    return rec


class LaneVerdictTests(unittest.TestCase):
    def verdict(self, boots, spec=None, readback=NONCE, cleanup=True):
        spec = MATRIX.lane_spec(spec or {"name": "cell", "storage": "sata", "boots": 2})
        res = {"boots": boots, "errors": [], "cleanup_confirmed": cleanup, "nonce": NONCE,
               "host_readback": readback}
        return MATRIX.lane_verdict(spec, res)

    def test_complete_two_boot_cell_passes(self):
        res = self.verdict([lane_boot_record(1), lane_boot_record(2)])
        self.assertEqual((res["status"], res["relay_input"], res["persistence"]), ("PASS", "PASS", "PASS"))

    def test_relay_without_input_reports_persistence_not_run(self):
        for first in (lane_boot_record(1, relay_input=False, nonce_written=False),
                      lane_boot_record(1, relay_input=None, nonce_written=None)):
            with self.subTest(first=first["relay_input"]):
                res = self.verdict([first])
                self.assertEqual(res["relay_input"], "FAIL")
                self.assertEqual(res["persistence"], "NOT-RUN")
                self.assertEqual(res["status"], "FAIL")

    def test_no_prompt_means_relay_input_not_run(self):
        res = self.verdict([lane_boot_record(1, prompt_seconds=None, relay_input=None)])
        self.assertEqual((res["relay_input"], res["persistence"], res["status"]), ("NOT-RUN", "NOT-RUN", "FAIL"))

    def test_persistence_needs_guest_and_host_read_back(self):
        cases = (([lane_boot_record(1), lane_boot_record(2, nonce_read=False)], NONCE),
                 ([lane_boot_record(1), lane_boot_record(2)], None),
                 ([lane_boot_record(1), lane_boot_record(2)], NONCE + "x"),
                 ([lane_boot_record(1, nonce_written=False)], NONCE))
        for boots, readback in cases:
            with self.subTest(boots=len(boots), readback=readback):
                res = self.verdict(boots, readback=readback)
                self.assertEqual((res["persistence"], res["status"]), ("FAIL", "FAIL"))

    def test_boot_markers_gate_a_cell(self):
        def without(text):
            rec = lane_boot_record(2)
            rec["markers"] = [line for line in rec["markers"] if text not in line]
            return rec
        cases = (without("board=x86_64-vm"), without("topology online="), without("block driver registered"),
                 without("/mnt/sd volume mounted"), without("machine pin: match"),
                 lane_boot_record(2, markers=lane_boot_record(2)["markers"] + ["Init: SAFE MODE — x"]),
                 lane_boot_record(2, rejected=["kernel panic"]), lane_boot_record(2, stopped=False),
                 lane_boot_record(2, errors=["power-off: x"]))
        for second in cases:
            with self.subTest(second=second):
                self.assertEqual(self.verdict([lane_boot_record(1), second])["status"], "FAIL")

    def test_topology_must_match_the_requested_vcpus(self):
        spec = {"name": "cell", "storage": "sata", "boots": 2, "cpus": 4}
        self.assertEqual(self.verdict([lane_boot_record(1), lane_boot_record(2)], spec)["status"], "FAIL")
        boots = [lane_boot_record(1, cpus=4), lane_boot_record(2, cpus=4)]
        self.assertEqual(self.verdict(boots, spec)["status"], "PASS")

    def test_cleanup_failure_fails_a_healthy_cell(self):
        self.assertEqual(self.verdict([lane_boot_record(1), lane_boot_record(2)], cleanup=False)["status"], "FAIL")

    def test_probe_is_observed_only_when_started_and_cleaned_up(self):
        spec = {"name": "probe", "storage": "sata", "viommu": True, "firmware": "efi", "probe": True}
        refused = {"boot": 1, "errors": ["vmrun start failed"], "prompt_seconds": None, "stopped": True}
        self.assertEqual(self.verdict([refused], spec)["status"], "OBSERVED")
        self.assertEqual(self.verdict([refused], spec, cleanup=False)["status"], "FAIL")
        self.assertEqual(self.verdict([], spec)["status"], "FAIL")

    def test_board_follows_the_profile(self):
        spec = {"name": "cell", "storage": "sata", "boots": 2, "profile": "pc"}
        boots = [lane_boot_record(1), lane_boot_record(2)]
        self.assertIn("board=x86_64-pc not reported", self.verdict(boots, spec)["boots"][0]["expectation_errors"])

    def test_two_boots_need_storage(self):
        with self.assertRaises(MATRIX.LaneError):
            MATRIX.lane_spec({"name": "cell", "boots": 2})

    def test_printed_lines_drop_echo_logs_and_prompt(self):
        raw = ("USER: vcat /mnt/sd/s3-lane.txt\n[ INFO] [vfs] read\nUSER: " + NONCE
               + "\nUSER: Init: service exited\nUSER: Cellos > \n").encode()
        self.assertEqual(MATRIX.printed_lines(raw, "vcat /mnt/sd/s3-lane.txt"), [NONCE])

    def test_marker_after_the_deadline_does_not_count(self):
        serial = MATRIX.RelaySerial.__new__(MATRIX.RelaySerial)
        serial.lock = MATRIX.threading.Lock()
        serial.data = bytearray(b"USER: Cellos > \n")
        self.assertFalse(serial.wait_for("Cellos >", 0, MATRIX.time.monotonic() - 1))
        self.assertTrue(serial.wait_for("Cellos >", 0, MATRIX.time.monotonic() + 1))
        self.assertFalse(serial.wait_for("Cellos >", len(serial.data), MATRIX.time.monotonic() + 0.2))


def net_boot_record(index, nic="vmxnet3", cpus=2, **override):
    """A matrix boot: storage markers plus the NIC bind/registration/lease and a guest S5."""
    rec = lane_boot_record(index, cpus=cpus)
    rec["markers"] = rec["markers"] + [
        "[ INFO] " + MATRIX.NIC_BIND[nic] + "x mac=00:0c:29:00:00:01 irq=msix",
        "[ WARN] [driver_cell] NIC driver registered: tid=12",
        "[net] DHCP acquired — IP configured"]
    rec.update({"guest_poweroff": True, "hard_stop": False})
    rec.update(override)
    return rec


class MatrixSweepTests(unittest.TestCase):
    RUN = "20261106T000356Z-87620f"

    def test_sweep_covers_the_grid_once_plus_one_bridge_cell_per_storage(self):
        cells = MATRIX.matrix_scenarios()
        names = [cell["name"] for cell in cells]
        self.assertEqual(len(names), len(set(names)))
        grid = [cell for cell in cells if not cell.get("legacy_bridge")]
        self.assertEqual(len(grid), 36)
        self.assertEqual({(c["nic"], c["storage"], c["cpus"], c["firmware"]) for c in grid},
                         set(MATRIX.itertools.product(("e1000", "e1000e", "vmxnet3"), ("sata", "nvme", "pvscsi"),
                                                      (2, 4), ("bios", "efi"))))
        bridged = [cell for cell in cells if cell.get("legacy_bridge")]
        self.assertEqual(sorted(cell["storage"] for cell in bridged), ["nvme", "pvscsi", "sata"])
        for cell in cells:
            with self.subTest(cell=cell["name"]):
                spec = MATRIX.lane_spec(dict(cell))
                self.assertEqual((spec["boots"], spec["memory_mib"], spec["net"], spec["poweroff"]),
                                 (2, 1024, True, True))

    def test_every_cell_name_fits_the_pipe_and_generates_its_vmx(self):
        for cell in MATRIX.matrix_scenarios():
            with self.subTest(cell=cell["name"]):
                pipe = rf"\\.\pipe\cellos-{self.RUN}-{cell['name']}-b2"
                self.assertTrue(MATRIX.VM.PIPE_NAME.fullmatch(pipe))
                keys = vmx_keys(MATRIX.VM.make_vmx(
                    "C:/t/c.iso", pipe, cell["cpus"], cell["memory_mib"], firmware=cell["firmware"],
                    nic=cell["nic"], storage=cell["storage"], disk="C:/t/disk.vmdk", pipe=True,
                    legacy_bridge=cell.get("legacy_bridge", False)))
                self.assertEqual((keys["memsize"], keys["firmware"], keys["ethernet0.virtualDev"]),
                                 ("1024", cell["firmware"], cell["nic"]))
                self.assertEqual(keys.get("pciBridge0.present"),
                                 "TRUE" if cell.get("legacy_bridge") else None)

    def test_only_selects_by_glob_and_refuses_an_empty_selection(self):
        cells = MATRIX.matrix_scenarios()
        self.assertEqual(MATRIX.select_scenarios(cells, []), cells)
        chosen = MATRIX.select_scenarios(cells, ["vmxnet3-*-efi"])
        self.assertEqual(len(chosen), 6)
        self.assertTrue(all(c["nic"] == "vmxnet3" and c["firmware"] == "efi" for c in chosen))
        self.assertEqual(len(MATRIX.select_scenarios(cells, ["bridge0-*", "e1000-sata-c2-bios"])), 4)
        with self.assertRaises(MATRIX.LaneError):
            MATRIX.select_scenarios(cells, ["igb-*"])

    def verdict(self, boots, nic="vmxnet3", readback=NONCE):
        spec = MATRIX.lane_spec({"name": "cell", "nic": nic, "storage": "pvscsi", "boots": 2,
                                 "net": True, "poweroff": True})
        res = {"boots": boots, "errors": [], "cleanup_confirmed": True, "nonce": NONCE,
               "host_readback": readback}
        return MATRIX.lane_verdict(spec, res)

    def test_complete_matrix_cell_passes_every_dimension(self):
        res = self.verdict([net_boot_record(1), net_boot_record(2)])
        self.assertEqual(res["status"], "PASS")
        self.assertEqual(res["dimensions"], {"prompt": "PASS", "network": "PASS",
                                             "persistence": "PASS", "poweroff": "PASS"})
        self.assertEqual(res["failures"], [])

    def test_lease_is_required_on_every_boot(self):
        second = net_boot_record(2)
        second["markers"] = [line for line in second["markers"] if "DHCP acquired" not in line]
        res = self.verdict([net_boot_record(1), second])
        self.assertEqual((res["status"], res["dimensions"]["network"], res["dimensions"]["persistence"]),
                         ("FAIL", "FAIL", "PASS"))
        self.assertIn("boot 2: missing marker: [net] DHCP acquired", res["failures"])

    def test_bind_must_be_the_cells_own_vmware_identity(self):
        for nic, other in (("e1000e", "e1000"), ("e1000", "vmxnet3"), ("vmxnet3", "e1000e")):
            with self.subTest(nic=nic):
                boots = [net_boot_record(1, nic=other), net_boot_record(2, nic=other)]
                self.assertEqual(self.verdict(boots, nic=nic)["dimensions"]["network"], "FAIL")

    def test_refusal_or_own_idle_fails_while_other_idle_cells_do_not(self):
        refusals = ("[platform] registration refused bus 3 device 0 function 0 id 15ad:07b0 class 2:0:0",
                    "[ WARN] [e1000] unsupported Ethernet 8086:100f; driver gate closed",
                    "[vmxnet3] no vmxnet3 controller present; driver cell idle")
        for line in refusals:
            with self.subTest(line=line):
                first = net_boot_record(1)
                first["markers"].append(line)
                res = self.verdict([first, net_boot_record(2)])
                self.assertEqual((res["status"], res["dimensions"]["network"]), ("FAIL", "FAIL"))
        first = net_boot_record(1)
        first["markers"].append("[e1000e] no 82574 controller present; driver cell idle")
        self.assertEqual(self.verdict([first, net_boot_record(2)])["status"], "PASS")
        first = net_boot_record(1, nic="e1000")
        first["markers"].append("[vmxnet3] no vmxnet3 controller present; driver cell idle")
        boots = [first, net_boot_record(2, nic="e1000")]
        self.assertEqual(self.verdict(boots, nic="e1000")["status"], "PASS")

    def test_hard_stop_fails_the_poweroff_dimension_only(self):
        res = self.verdict([net_boot_record(1), net_boot_record(2, guest_poweroff=False, hard_stop=True)])
        self.assertEqual(res["status"], "FAIL")
        self.assertEqual((res["dimensions"]["poweroff"], res["dimensions"]["network"],
                          res["dimensions"]["persistence"]), ("FAIL", "PASS", "PASS"))
        self.assertIn("boot 2: no guest S5 power-off: hard stop", res["failures"])

    def test_missing_second_boot_fails_prompt_and_network(self):
        res = self.verdict([net_boot_record(1, nonce_written=False)])
        self.assertEqual((res["dimensions"]["prompt"], res["dimensions"]["network"],
                          res["dimensions"]["persistence"]), ("FAIL", "FAIL", "FAIL"))

    def test_nic_verdict_needs_a_nic(self):
        with self.assertRaises(MATRIX.LaneError):
            MATRIX.lane_spec({"name": "cell", "net": True})

    def test_matrix_table_counts_each_dimension(self):
        good = self.verdict([net_boot_record(1), net_boot_record(2)])
        good["scenario"] = "a"
        bad = self.verdict([net_boot_record(1), net_boot_record(2, hard_stop=True)])
        bad["scenario"] = "b"
        table = MATRIX.matrix_table([good, bad])
        self.assertEqual([row["cell"] for row in table["cells"]], ["a", "b"])
        self.assertIsNone(table["cells"][0]["decisive"])
        self.assertEqual(table["cells"][1]["decisive"], "boot 2: no guest S5 power-off: hard stop")
        self.assertEqual(table["totals"]["status"], {"PASS": 1, "FAIL": 1})
        self.assertEqual(table["totals"]["network"], {"PASS": 2})

    def test_jobs_run_cells_side_by_side_and_keep_matrix_order(self):
        started, lock = [], MATRIX.threading.Lock()
        def fake_run_lane(vmrun, vdisk, iso, spec, run_id, evidence_root):
            with lock:
                started.append(spec["name"])
            MATRIX.time.sleep(0.05 if spec["name"].endswith("bios") else 0.01)
            return MATRIX.lane_verdict(MATRIX.lane_spec(spec), {
                "scenario": spec["name"], "boots": [net_boot_record(1, nic=spec["nic"], cpus=spec["cpus"]),
                                                    net_boot_record(2, nic=spec["nic"], cpus=spec["cpus"])],
                "errors": [], "cleanup_confirmed": True, "nonce": NONCE, "host_readback": NONCE})
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            iso = root / "vm.iso"
            iso.write_bytes(b"iso")
            args = MATRIX.argparse.Namespace(command="matrix", iso=str(iso), pc_iso=str(iso),
                                             memory_mib=1024, window=120.0, json=None, jobs=3,
                                             only=["e1000-*"])
            with patch.object(MATRIX.VM, "find_vmrun", return_value=root / "vmrun.exe"), \
                 patch.object(MATRIX.VM, "IS_WSL_WIN", True), \
                 patch.object(MATRIX.VM, "to_host_path", side_effect=str), \
                 patch.object(MATRIX, "EVIDENCE_ROOT", root / "evidence"), \
                 patch.object(MATRIX, "LANE_ROOT", root / "lane"), \
                 patch.object(MATRIX.subprocess, "run",
                              return_value=MATRIX.subprocess.CompletedProcess([], 0, "", "")), \
                 patch.object(MATRIX, "run_lane", side_effect=fake_run_lane), \
                 patch("builtins.print"):
                self.assertEqual(MATRIX.lane_main(args), 0)
            summary = MATRIX.json.loads(next((root / "evidence").glob("*/summary.json")).read_text())
        expected = [c["name"] for c in MATRIX.matrix_scenarios() if c["name"].startswith("e1000-")]
        self.assertEqual(len(expected), 12)
        self.assertEqual(sorted(started), sorted(expected))
        self.assertEqual([row["cell"] for row in summary["matrix"]["cells"]], expected)
        self.assertEqual(summary["matrix"]["totals"]["status"], {"PASS": 12})
        self.assertEqual(summary["jobs"], 3)


RELAY_SPEC = importlib.util.spec_from_file_location(
    "vmware_serial_relay", Path(__file__).with_name("vmware_serial_relay.py"))
RELAY = importlib.util.module_from_spec(RELAY_SPEC)
RELAY_SPEC.loader.exec_module(RELAY)


class FakeKernel32:
    """PeekNamedPipe/ReadFile/WriteFile over in-memory bytes; writes land 3 bytes at a time."""

    def __init__(self, incoming):
        self.incoming = bytearray(incoming)
        self.written = bytearray()

    def CreateFileW(self, *args):
        return 7

    def PeekNamedPipe(self, handle, buffer, size, read, available, left):
        available._obj.value = len(self.incoming)
        return True

    def ReadFile(self, handle, buffer, size, done, overlapped):
        chunk = self.incoming[:size]
        del self.incoming[:size]
        RELAY.ctypes.memmove(buffer, bytes(chunk), len(chunk))
        done._obj.value = len(chunk)
        return True

    def WriteFile(self, handle, data, size, done, overlapped):
        self.written += data[:3]
        done._obj.value = min(3, size)
        return True


class RelayFramingTests(unittest.TestCase):
    def test_relay_and_vmx_accept_the_same_pipe_names(self):
        for name in (PIPE, r"\\.\pipe\cellos-20261105T233037Z-e70694-bios-sata-c2-b1",
                     r"\\.\pipe\other", r"\\.\pipe\cellos-a\b", "C:/t/serial.log"):
            with self.subTest(name=name):
                self.assertEqual(bool(RELAY.PIPE_NAME.fullmatch(name)),
                                 bool(MATRIX.VM.PIPE_NAME.fullmatch(name)))

    def test_reads_exactly_the_waiting_bytes_and_writes_everything(self):
        k32 = FakeKernel32(b"USER: Cellos > \x00\xff")
        pipe = RELAY.Pipe(k32, PIPE, 1)
        count = pipe.available()
        self.assertEqual(pipe.read(count), b"USER: Cellos > \x00\xff")
        self.assertEqual(pipe.available(), 0)
        pipe.write(b"uname -a\n")
        self.assertEqual(bytes(k32.written), b"uname -a\n")


if __name__ == "__main__":
    unittest.main()
