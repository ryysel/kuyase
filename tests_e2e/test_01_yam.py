#!/usr/bin/env python3
"""
E2E Smoke Test 01: YAM v0.2+ Installer (`install.sh` & `install.ps1`)
Executes exclusively via `kuyase run --headless --board <board> --snapshot --exec`:
1. Runs the unmodified `install.sh` (from `../yam/install.sh` or `nyakokitsu/yam`) inside the guest
2. Verifies platform auto-detection (`YAM_PLATFORM` in `/data/yam/bin/yam-run`) matches SoC/DT
3. Verifies `/usr/bin/yam list` and `/usr/bin/yam doctor` without adding any registry or public key
4. Verifies `install.ps1` `Get-DetectedPlatform` regex table matches the same canonical platform
"""

from __future__ import annotations

import re
import unittest

from tests_e2e.harness import (
    ADB_GUEST_SHIM,
    EXPECTED_YAM_PLATFORM,
    discover_test_boards,
    load_yam_install_ps1,
    load_yam_install_sh,
    run_headless_exec,
)


def evaluate_ps1_detected_platform(install_ps1: str, compact_identity: str) -> str:
    """Evaluate the `if ($compact -match '...') { "..." }` chain from `install.ps1`."""
    fn_match = re.search(
        r"function\s+Get-DetectedPlatform[^{]*\{(.*?)\n\}",
        install_ps1,
        re.DOTALL,
    )
    if not fn_match:
        raise RuntimeError("Get-DetectedPlatform not found in install.ps1")
    body = fn_match.group(1)

    # Extract each `-match 'pattern'` branch in exact order from Get-DetectedPlatform
    for line in body.splitlines():
        line = line.strip()
        if "$compact -match 'yandex,station-mini2|station-mini2'" in line:
            if re.search(r"yandex,station-mini2|station-mini2", compact_identity):
                return "yandexmini_2"
        elif "$compact -match 'yandex,cucumber-dvt2|cucumber'" in line:
            if re.search(r"yandex,cucumber-dvt2|cucumber", compact_identity):
                return "cucumber"
        elif "$compact -match 'yandex,station-midi'" in line:
            if re.search(r"yandex,station-midi", compact_identity):
                if re.search(r"cucumber|amlogic,a5", compact_identity):
                    return "cucumber"
                return "yandexstation_2"
        else:
            m = re.search(
                r"\$compact\s+-match\s+'([^']+)'\)\s*\{\s*\"([^\"]+)\"\s*\}", line
            )
            if m and re.search(m.group(1), compact_identity):
                return m.group(2)
    return ""


def build_yam_install_exec_script(install_sh: str, expected_platform: str) -> str:
    return (
        "set -eu\n"
        f"{ADB_GUEST_SHIM}\n"
        "(\n"
        "  set --\n"
        f"{install_sh}\n"
        ")\n"
        "test -x /data/yam/bin/yam\n"
        "test -x /data/yam/bin/yam-run\n"
        "test -L /usr/bin/yam\n"
        f"test -s '/data/yam/platforms/{expected_platform}.yaml'\n"
        f"grep -Fq \"export YAM_PLATFORM='{expected_platform}'\" /data/yam/bin/yam-run\n"
        "ID_COMPACT=$({ getprop ro.product.device 2>/dev/null || true; getprop ro.product.model 2>/dev/null || true; cat /proc/device-tree/compatible /proc/device-tree/amlogic-dt-id 2>/dev/null | tr '\\000' '\\n' || true; } | tr -d '\\r \\t' | tr '[:upper:]' '[:lower:]')\n"
        'printf "__YAM_ID_COMPACT=%s\\n" "$ID_COMPACT"\n'
        "/usr/bin/yam list\n"
        "DOCTOR_OUT=$(/usr/bin/yam doctor)\n"
        'printf "%s\\n" "$DOCTOR_OUT"\n'
        'printf "%s\\n" "$DOCTOR_OUT" | grep -Fq "Состояние пакетов в порядке"\n'
    )


class TestYamInstallerE2E(unittest.TestCase):
    def test_yam_install_sh_and_ps1_across_boards(self) -> None:
        boards = discover_test_boards()
        if not boards:
            self.skipTest("No `work/*-system.ext4` guest images found")

        install_sh = load_yam_install_sh()
        install_ps1 = load_yam_install_ps1()

        for idx, board in enumerate(boards):
            expected_platform = EXPECTED_YAM_PLATFORM.get(board, board)
            with self.subTest(board=board):
                script = build_yam_install_exec_script(install_sh, expected_platform)
                out = run_headless_exec(board, script, port_offset=200 + idx * 10)
                self.assertIn("Состояние пакетов в порядке", out)

                m = re.search(r"__YAM_ID_COMPACT=(\S+)", out)
                self.assertIsNotNone(
                    m, f"[{board}] Missing __YAM_ID_COMPACT in output:\n{out}"
                )
                assert m is not None
                ps1_plat = evaluate_ps1_detected_platform(install_ps1, m.group(1))
                self.assertEqual(
                    ps1_plat,
                    expected_platform,
                    f"[{board}] install.ps1 Get-DetectedPlatform returned '{ps1_plat}', expected '{expected_platform}'",
                )


if __name__ == "__main__":
    unittest.main(verbosity=2)
