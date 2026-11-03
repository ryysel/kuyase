#!/usr/bin/env python3
"""
E2E Smoke Test 02: Luna Runtime (`nyakokitsu/luna`) Installer (`install.sh` & `install.ps1`)
Executes exclusively via `kuyase run --headless --board <board> --snapshot --exec`:
1. Runs the unmodified `nyakokitsu/luna/install.sh` inside the guest with canonical `uname -n`
   (verifying station auto-detection without `--platform`)
2. Executes the embedded Python manifest resolver directly from `nyakokitsu/luna/install.sh`
   and verifies `nyakokitsu/luna/install.ps1` `Get-DetectedDtPlatform` against the guest DT
3. Verifies `/etc/init.d/S99luna` starts `/data/quazar/luna/bin/luna` on `127.0.0.1:8099`
4. Verifies `GET /api/device`, `POST /api/led/ring`, `POST /api/led/off`, `POST /api/clock`, and `POST /api/volume/set`
   (while `kuyase --exec` enforces `ipc_error_count == 0` and `led_trigger_conflict_count == 0`)
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import tempfile
import unittest
from typing import Optional, Tuple

from tests_e2e.harness import (
    ADB_GUEST_SHIM,
    CANONICAL_STATION_HOSTNAME,
    EXPECTED_YAM_PLATFORM,
    discover_test_boards,
    http_get_text,
    load_luna_install_ps1,
    load_luna_install_sh,
    run_headless_exec,
)


def run_install_sh_embedded_manifest_resolver(
    install_sh: str, manifest_raw: str, target: str
) -> Optional[Tuple[str, str, str]]:
    """Run the exact embedded `python3 - ... <<'PY'` script from `luna/install.sh`."""
    m = re.search(
        r"python3\s+-\s+[^\n]*<<'PY'\n(.*?)\nPY",
        install_sh,
        re.DOTALL,
    )
    if not m:
        raise RuntimeError("Embedded Python manifest resolver not found in luna/install.sh")
    embedded_py = m.group(1)

    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", suffix=".json", delete=False
    ) as tmp:
        tmp.write(manifest_raw)
        tmp_path = pathlib.Path(tmp.name)

    try:
        proc = subprocess.run(
            [sys.executable, "-c", embedded_py, str(tmp_path), target, "0"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=15,
        )
        if proc.returncode != 0:
            return None
        parts = proc.stdout.rstrip("\r\n").split("\t")
        if len(parts) < 2:
            return None
        device = parts[0]
        asset = parts[1]
        codename = parts[2] if len(parts) > 2 else ""
        return (device, asset, codename)
    finally:
        tmp_path.unlink(missing_ok=True)


def evaluate_luna_ps1_detected_dt_platform(install_ps1: str, dt_compact: str) -> str:
    """Evaluate `Get-DetectedDtPlatform` regex branches directly from `luna/install.ps1`."""
    fn_match = re.search(
        r"function\s+Get-DetectedDtPlatform[^{]*\{(.*?)\n\}",
        install_ps1,
        re.DOTALL,
    )
    if not fn_match:
        return ""
    for line in fn_match.group(1).splitlines():
        m = re.search(
            r"\$compact\s+-match\s+'([^']+)'\)\s*\{\s*return\s+\"([^\"]+)\"\s*\}",
            line.strip(),
        )
        if m and re.search(m.group(1), dt_compact):
            return m.group(2)
    return ""


def build_luna_install_and_api_exec_script(
    install_sh: str,
    canonical_hostname: str,
    device: str,
    asset: str,
    codename: str,
    board: str,
) -> str:
    # Provide `uname` (returning canonical station hostname for `-n`) and `python3`
    # (forwarding the output of `luna/install.sh`'s own embedded Python resolver)
    # so the unmodified `nyakokitsu/luna/install.sh` runs inside the guest `/bin/sh`.
    sysfs_led_boards = (
        "cucumber",
        "yandexmini2",
        "yandexmini2nc",
        "plum",
        "plumnc",
        "yandexmicro",
        "mango",
    )
    switch_trigger = (
        'for t in /sys/class/leds/*:ring*/trigger; do [ -e "$t" ] && echo none > "$t" 2>/dev/null || true; done'
        if board in sysfs_led_boards
        else ":"
    )

    return (
        "set -eu\n"
        f"{ADB_GUEST_SHIM}\n"
        "uname() {\n"
        '  if [ "${1:-}" = "-n" ]; then\n'
        f'    printf "%s\\n" "{canonical_hostname}"\n'
        "    return 0\n"
        "  fi\n"
        '  command uname "$@"\n'
        "}\n"
        "python3() {\n"
        "  cat >/dev/null\n"
        f'  printf "%s\\t%s\\t%s\\n" "{device}" "{asset}" "{codename}"\n'
        "}\n"
        "(\n"
        "  set --\n"
        f"{install_sh}\n"
        ")\n"
        "test -x /data/quazar/luna/bin/luna\n"
        "test -x /etc/init.d/S99luna\n"
        "DT_COMPACT=$(cat /proc/device-tree/compatible /proc/device-tree/amlogic-dt-id 2>/dev/null | tr '\\000' '\\n' | tr -d '\\r \\t' | tr '[:upper:]' '[:lower:]')\n"
        'printf "__LUNA_DT_COMPACT=%s\\n" "$DT_COMPACT"\n'
        "i=0\n"
        "while [ $i -lt 30 ]; do\n"
        "  if command wget -qO- http://127.0.0.1:8099/api/device >/tmp/luna-device.json 2>/dev/null; then\n"
        "    break\n"
        "  fi\n"
        "  i=$((i + 1))\n"
        "  sleep 1\n"
        "done\n"
        'grep -Fq \'"model"\' /tmp/luna-device.json\n'
        f"{switch_trigger}\n"
        "RING_COUNT=$(sed -n 's/.*\"ring_leds\"[[:space:]]*:[[:space:]]*\\([0-9]*\\).*/\\1/p' /tmp/quasar-emu-hw.json | head -n 1)\n"
        "COLORS=$(awk -v n=\"${RING_COUNT:-6}\" 'BEGIN { printf \"[\"; for (i=0; i<n; i++) printf \"%s\\\"#22CCFF\\\"\", (i?\",\":\"\"); printf \"]\" }')\n"
        "command wget -qO- --header='Content-Type: application/json' --header='X-Luna-Local: 1' "
        "--post-data=\"{\\\"colors\\\":$COLORS}\" http://127.0.0.1:8099/api/led/ring >/dev/null\n"
        "command wget -qO- --header='Content-Type: application/json' --header='X-Luna-Local: 1' "
        "--post-data='{}' http://127.0.0.1:8099/api/led/off >/dev/null\n"
        "if grep -Fq '\"has_clock\": true' /tmp/quasar-emu-hw.json; then\n"
        "  command wget -qO- --header='Content-Type: application/json' --header='X-Luna-Local: 1' "
        "--post-data='{\"time\":\"12:34\",\"brightness\":200}' http://127.0.0.1:8099/api/clock >/dev/null\n"
        "fi\n"
        "command wget -qO- --header='Content-Type: application/json' --header='X-Luna-Local: 1' "
        "--post-data='{\"percent\":70}' http://127.0.0.1:8099/api/volume/set >/dev/null\n"
        "printf '__LUNA_E2E_OK__\\n'\n"
    )


class TestLunaInstallerAndRuntimeE2E(unittest.TestCase):
    def test_luna_install_sh_ps1_and_peripherals_across_boards(self) -> None:
        boards = discover_test_boards()
        if not boards:
            self.skipTest("No `work/*-system.ext4` guest images found")

        install_sh = load_luna_install_sh()
        install_ps1 = load_luna_install_ps1()
        manifest_raw = http_get_text(
            "https://github.com/nyakokitsu/luna/releases/latest/download/manifest.json"
        )

        for idx, board in enumerate(boards):
            canonical_host = CANONICAL_STATION_HOSTNAME.get(board, board)
            matched = run_install_sh_embedded_manifest_resolver(
                install_sh, manifest_raw, canonical_host
            )
            with self.subTest(board=board):
                if matched is None:
                    self.skipTest(
                        f"[{board}] Not listed in nyakokitsu/luna release manifest.json"
                    )
                device, asset, codename = matched
                script = build_luna_install_and_api_exec_script(
                    install_sh=install_sh,
                    canonical_hostname=canonical_host,
                    device=device,
                    asset=asset,
                    codename=codename,
                    board=board,
                )
                out = run_headless_exec(board, script, port_offset=300 + idx * 10)
                self.assertIn("__LUNA_E2E_OK__", out)

                dt_match = re.search(r"__LUNA_DT_COMPACT=(\S+)", out)
                if dt_match and "function Get-DetectedDtPlatform" in install_ps1:
                    expected_dt_plat = EXPECTED_YAM_PLATFORM.get(board, board)
                    ps1_dt_plat = evaluate_luna_ps1_detected_dt_platform(
                        install_ps1, dt_match.group(1)
                    )
                    self.assertEqual(ps1_dt_plat, expected_dt_plat)


if __name__ == "__main__":
    unittest.main(verbosity=2)
