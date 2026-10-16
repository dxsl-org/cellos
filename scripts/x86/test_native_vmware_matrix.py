"""Runner verdict regressions; synthetic VM outcomes are not native runtime evidence."""
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


if __name__ == "__main__":
    unittest.main()
