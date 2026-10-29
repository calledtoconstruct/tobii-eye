import unittest
from pathlib import Path

_README = Path(__file__).resolve().parents[1] / "README.md"
_SERIAL = "IS5FF-100204240211"


class ReadmeTest(unittest.TestCase):
    def test_readme_omits_unit_serial_and_machine_phrase(self) -> None:
        text = _README.read_text(encoding="utf-8")
        self.assertNotIn(_SERIAL, text)
        self.assertNotIn("on this machine", text)
