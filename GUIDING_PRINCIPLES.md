# Guiding principles: calledtoconstruct/tobii-eye

## Purpose and scope
Open-source Linux tools for the Tobii Eye Tracker 5: a Rust gaze daemon, a Python client and calibration, udev rules and Arch packaging.

## Stack and layout
Rust daemon in `daemon/` (`tobiifreed`, GPL-3.0-only, libusb). Python package `tobii-input` in `src/` (setuptools, Python >=3.14.9, MIT) with tests in `tests/`. `udev/`, `packaging/` (PKGBUILDs), `scripts/`, `docs/`. The Python client must not link the GPL daemon.

## Commands
| Check | Command | Evidence |
|---|---|---|
| Build (daemon) | `cargo build --release --manifest-path daemon/Cargo.toml` | README.md |
| Build (Python) | `python -m build` | `.gitlab-ci.yml` |
| Type-check / lint | None configured | - |
| Changed-feature tests | `PYTHONPATH=src python -m unittest tests.test_<module> -v` | `.gitlab-ci.yml` (unittest discover) |
| Full suite | `PYTHONPATH=src python -m unittest discover -s tests -v` | `.gitlab-ci.yml` |
| CI | `.gitlab-ci.yml` runs test/build on GitLab merge requests; there are no GitHub workflows, so a GitHub PR gets no CI | `.gitlab-ci.yml` |

Commands marked as not written down come from the build files, not from project docs; confirm them before relying on them.

## Before every push
Before every push: run the build and type-check/lint that exist here, plus only the tests for the files or feature you changed (team rule, ideas #14 note 6797). Do not run the full suite locally as a routine gate; this repository has no CI that runs the full suite, so run the full suite only when the change is broad, and say in the PR that CI coverage is missing. Merge needs a positive review. Explain any new failure your change caused. Never weaken a valid test to make it pass, and never invent a command this file does not list.

## Project rules
- Keep the license split: the daemon is GPL-3.0-only, the Python client is MIT and talks to it only over the socket.
- Nothing is tied to one unit or one screen; keep device and screen values configurable.
- `udev/60-tobii-eyetracker.rules` and the packaged rule are the same file and must sort before `73-seat-late.rules`.

## What to avoid
- Committing to the default branch; work on a branch and open a pull request.
- Deleting files, branches or history, or changing repository settings.
- Adding tooling, CI or dependencies that the change does not need.
- Rules copied from other projects (file-size limits, clocks, quotas) that this repository never adopted.

## Delivery and safety
- Release builds come from `vX.Y.Z` tags on GitLab; do not create tags in a routine change.

## Open questions
- GitHub has no CI for this repository.

## Sources
Read from the `main` branch on 2026-11-08 ET: `README.md`, `pyproject.toml`, `daemon/Cargo.toml`, `.gitlab-ci.yml`.

## Instruction updates
Check `guiding-principles.version` at the start of each turn and when you enter this project; reload this file when the version changes. Change this file, its version and date, and `PRINCIPLES_HISTORY.md` in the same commit. Team-wide role instructions are versioned separately in the team-process repository.
