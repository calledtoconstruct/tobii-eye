import os
import shutil
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


def _run_find(
    sysfs: Path,
    dev_root: Path,
    *,
    path: str | None = None,
    extra_env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    # The installer calls sudo. This runs only the lookup.
    env = {
        **os.environ,
        "TOBII_SYSFS": str(sysfs),
        "TOBII_DEV_ROOT": str(dev_root),
    }
    if path is not None:
        env["PATH"] = path
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        [str(_FIND)],
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )


def _layout(root: Path) -> tuple[Path, Path, tuple[Path, Path], tuple[Path, Path]]:
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
    return sysfs, dev_root, matched, skipped


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
            sysfs, dev_root, matched, skipped = _layout(root)
            # Different modes, so a canned ACL line cannot satisfy both nodes.
            matched[0].chmod(0o660)
            matched[1].chmod(0o666)
            proc = _run_find(sysfs, dev_root)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertNotIn("no Tobii device plugged in", proc.stdout)
        self.assertNotIn("getfacl failed", proc.stdout)
        self.assertNotIn("user::fixture", proc.stdout)
        # `# file:` is getfacl -p naming the path it opened.
        self.assertEqual(proc.stdout.count("# file: "), 2)
        for node in matched:
            self.assertIn(str(node), proc.stdout)
            self.assertIn(f"# file: {node}\n", proc.stdout)
        self.assertIn("user::rw-\ngroup::rw-\nother::---\n", proc.stdout)
        self.assertIn("user::rw-\ngroup::rw-\nother::rw-\n", proc.stdout)
        for node in skipped:
            self.assertNotIn(str(node), proc.stdout)
        self.assertNotIn("/dev/bus/usb/001/002", proc.stdout)

    def test_getfacl_failure_is_visible(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            sysfs, dev_root, matched, skipped = _layout(root)
            bin_dir = root / "bin"
            bin_dir.mkdir()
            tr = shutil.which("tr")
            self.assertIsNotNone(tr)
            os.symlink(tr, bin_dir / "tr")
            missing = _run_find(sysfs, dev_root, path=str(bin_dir))
        self.assertEqual(missing.returncode, 0, missing.stderr)
        self.assertIn("not found", missing.stderr)
        self.assertNotIn("user::", missing.stdout)
        for node in matched:
            self.assertIn(f"getfacl failed for {node}", missing.stdout)
            self.assertNotIn(f"# file: {node}", missing.stdout)
        for node in skipped:
            self.assertNotIn(str(node), missing.stdout)

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            sysfs, dev_root, matched, skipped = _layout(root)
            log = root / "getfacl.log"
            bin_dir = root / "bin"
            bin_dir.mkdir()
            stub = bin_dir / "getfacl"
            _write(
                stub,
                "#!/bin/sh\n"
                'printf "%s %s %s\\n" "$1" "$2" "$3" >> "$GETFACL_LOG"\n'
                "exit 1\n",
            )
            stub.chmod(0o755)
            failed = _run_find(
                sysfs,
                dev_root,
                path=str(bin_dir) + os.pathsep + os.environ.get("PATH", ""),
                extra_env={"GETFACL_LOG": str(log)},
            )
            calls = log.read_text(encoding="utf-8").splitlines()
        self.assertEqual(failed.returncode, 0, failed.stderr)
        self.assertEqual(sorted(calls), sorted(f"-p -- {node}" for node in matched))
        self.assertNotIn("user::", failed.stdout)
        for node in matched:
            self.assertIn(f"getfacl failed for {node}", failed.stdout)
        for node in skipped:
            self.assertNotIn(str(node), failed.stdout)

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
