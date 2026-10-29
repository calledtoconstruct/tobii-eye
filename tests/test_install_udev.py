import os
import subprocess
import tempfile
import unittest
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[1]
_FIND = _ROOT / "scripts" / "find-tobii-usb.sh"
_INSTALL = _ROOT / "scripts" / "install-udev.sh"
_RULE_NAME = "60-tobii-eyetracker.rules"
_UDEV_RULE = _ROOT / "udev" / _RULE_NAME
_PACKAGE_RULE = _ROOT / "packaging" / "tobiifreed" / _RULE_NAME


def _write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _sysfs_device(
    sysfs: Path, name: str, vendor: str, product: str, bus: str, num: str
) -> None:
    dev = sysfs / "bus" / "usb" / "devices" / name
    _write(dev / "idVendor", vendor + "\n")
    _write(dev / "idProduct", product + "\n")
    _write(dev / "busnum", bus + "\n")
    _write(dev / "devnum", num + "\n")


def _run_find(sysfs: Path, dev_root: Path) -> subprocess.CompletedProcess[str]:
    # The installer calls sudo. This runs only the lookup.
    return subprocess.run(
        [str(_FIND)],
        check=False,
        capture_output=True,
        text=True,
        env={
            **os.environ,
            "TOBII_SYSFS": str(sysfs),
            "TOBII_DEV_ROOT": str(dev_root),
        },
    )


class InstallUdevTest(unittest.TestCase):
    def test_no_device_says_so_and_exits_zero(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            sysfs = root / "sys"
            _sysfs_device(sysfs, "1-1", "1d6b", "0002", "1", "1")
            proc = _run_find(sysfs, root / "dev")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("no Tobii device plugged in", proc.stdout)
        self.assertNotIn("user::", proc.stdout)
        self.assertNotIn("/dev/bus/usb/001/002", proc.stdout)

    def test_fixture_prints_acl_for_both_product_ids(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            sysfs = root / "sys"
            dev_root = root / "dev"
            _sysfs_device(sysfs, "2-4", "2104", "0313", "2", "7")
            _sysfs_device(sysfs, "4-1", "2104", "0102", "4", "9")
            _sysfs_device(sysfs, "1-3", "2104", "ffff", "1", "4")
            _sysfs_device(sysfs, "3-1", "1234", "0313", "3", "2")
            _write(sysfs / "bus" / "usb" / "devices" / "2-4:1.0" / "idVendor", "2104\n")
            _write(sysfs / "bus" / "usb" / "devices" / "2-4:1.0" / "idProduct", "0313\n")
            matched = (
                dev_root / "bus" / "usb" / "002" / "007",
                dev_root / "bus" / "usb" / "004" / "009",
            )
            skipped = (
                dev_root / "bus" / "usb" / "001" / "004",
                dev_root / "bus" / "usb" / "003" / "002",
            )
            for node in matched + skipped:
                _write(node, "")
            proc = _run_find(sysfs, dev_root)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertNotIn("no Tobii device plugged in", proc.stdout)
        for node in matched:
            self.assertIn(str(node), proc.stdout)
        for node in skipped:
            self.assertNotIn(str(node), proc.stdout)
        self.assertNotIn("/dev/bus/usb/001/002", proc.stdout)
        self.assertEqual(proc.stdout.count("user::"), 2)

    def test_install_triggers_by_vendor_without_hardcoded_bus(self) -> None:
        text = _INSTALL.read_text(encoding="utf-8")
        self.assertNotIn("/dev/bus/usb/", text)
        self.assertIn("find-tobii-usb.sh", text)
        triggers = [line for line in text.splitlines() if "udevadm trigger" in line]
        self.assertEqual(triggers, [
            "sudo udevadm trigger --subsystem-match=usb --attr-match=idVendor=2104",
        ])
        finder = _FIND.read_text(encoding="utf-8")
        self.assertNotIn("sudo", finder)
        self.assertNotIn("001/002", finder)

    def test_one_udev_rule_matches_package_without_group(self) -> None:
        self.assertTrue(_PACKAGE_RULE.is_file())
        self.assertFalse(_PACKAGE_RULE.is_symlink())
        self.assertEqual(_UDEV_RULE.resolve(), _PACKAGE_RULE.resolve())
        text = _PACKAGE_RULE.read_text(encoding="utf-8")
        self.assertEqual(_UDEV_RULE.read_text(encoding="utf-8"), text)
        self.assertNotIn("GROUP", text)
        self.assertEqual(
            [line for line in text.splitlines() if line.startswith("SUBSYSTEM==")],
            [
                'SUBSYSTEM=="usb", ATTR{idVendor}=="2104", ATTR{idProduct}=="0313", MODE="0660", TAG+="uaccess"',
                'SUBSYSTEM=="usb", ATTR{idVendor}=="2104", ATTR{idProduct}=="0102", MODE="0660", TAG+="uaccess"',
            ],
        )
