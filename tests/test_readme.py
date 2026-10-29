import unittest
from pathlib import Path

_README = Path(__file__).resolve().parents[1] / "README.md"
_SERIAL = "IS5FF-100204240211"


class ReadmeTest(unittest.TestCase):
    def test_readme_omits_unit_serial_and_machine_phrase(self) -> None:
        text = _README.read_text(encoding="utf-8")
        self.assertNotIn(_SERIAL, text)
        self.assertNotIn("on this machine", text)

    def test_readme_states_both_transports(self) -> None:
        text = _README.read_text(encoding="utf-8")
        self.assertIn("$XDG_RUNTIME_DIR/tobiifreed/gaze.sock", text)
        self.assertIn("ws://127.0.0.1:7081", text)
        self.assertIn("--ws", text)
        self.assertIn("only when `--ws` is set", text)
