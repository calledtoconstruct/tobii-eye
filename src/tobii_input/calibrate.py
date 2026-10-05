"""Five-point calibration. Space starts, Esc quits, P plays Pop afterward."""

from tobii_input.desktop import run


def main() -> None:
    run("calibrate")


if __name__ == "__main__":
    main()
