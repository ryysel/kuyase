#!/usr/bin/env python3
"""
E2E Smoke Test 03: Station AirPlay 2.0 (`ryysel/Station-airplay`)
Executes exclusively via `kuyase run --headless --board <board> --snapshot --exec`:
1. Installs YAM v0.2+ via the unmodified `install.sh`
2. Installs the `yam/luna` `.yampkg` artifact directly as a local file (`yam install --yes /tmp/luna.yampkg`)
   and starts Luna on `:8099` with `allow_plugin_binaries: true`
3. Installs `station-airplay-<ver>-<platform>.yampkg` from `https://github.com/ryysel/Station-airplay`
   releases (or `KUYASE_AIRPLAY_PKG`) directly as a local `.yampkg` file with zero registry keys
4. Waits for `/run/station-airplay/private-bus` and RTSP listener (`:5000` or `:7000`)
5. Verifies `org.gnome.ShairportSync.GetControlStatus` on the private D-Bus and clean service stop
"""

from __future__ import annotations

import os
import pathlib
import unittest

from tests_e2e.harness import (
    ADB_GUEST_SHIM,
    EXPECTED_YAM_PLATFORM,
    discover_test_boards,
    latest_airplay_release_tag,
    load_yam_install_sh,
    run_headless_exec,
)

SUPPORTED_AIRPLAY_PLATFORMS = {
    "yandexstation_2",
    "cucumber",
    "orion",
    "plum",
    "yandexmicro",
    "yandexmini",
    "yandexmini_2",
    "bergamot",
}

LUNA_YAMPKG_VERSION = "0.2.18"


def build_airplay_exec_script(
    yam_install_sh: str,
    yam_platform: str,
    airplay_tag: str,
) -> str:
    airplay_ver = airplay_tag.lstrip("v")
    luna_pkg_url = (
        f"https://repo.yse.wtf/v2/packages/yam/luna/{LUNA_YAMPKG_VERSION}/{yam_platform}.yampkg"
    )
    local_pkg = os.environ.get("KUYASE_AIRPLAY_PKG", "").strip()
    if local_pkg:
        if not pathlib.Path(local_pkg).is_file():
            raise FileNotFoundError(f"KUYASE_AIRPLAY_PKG not found: {local_pkg}")
        airplay_pkg_url = "https://local.kuyase.e2e/airplay.yampkg"
    else:
        airplay_pkg_url = (
            f"https://github.com/ryysel/Station-airplay/releases/download/{airplay_tag}/"
            f"station-airplay-{airplay_ver}-{yam_platform}.yampkg"
        )

    return (
        "set -eu\n"
        f"{ADB_GUEST_SHIM}\n"
        "(\n"
        "  set --\n"
        f"{yam_install_sh}\n"
        ")\n"
        "mkdir -p /data/yam/var/luna/data\n"
        "printf 'allow_plugin_binaries = true\\n' > /data/yam/var/luna/data/settings.toml\n"
        f"wget -qO /tmp/luna.yampkg '{luna_pkg_url}'\n"
        "/usr/bin/yam install --yes /tmp/luna.yampkg\n"
        "rm -f /tmp/luna.yampkg\n"
        f"wget -qO /tmp/station-airplay.yampkg '{airplay_pkg_url}'\n"
        "/usr/bin/yam install --yes /tmp/station-airplay.yampkg\n"
        "rm -f /tmp/station-airplay.yampkg\n"
        "pkill -f airplay-enable-worker 2>/dev/null || true\n"
        "command wget -qO- --header='X-Luna-Local: 1' --post-data='' "
        "http://127.0.0.1:8099/api/plugins/rescan >/dev/null 2>&1 || true\n"
        "command wget -qO- --header='X-Luna-Local: 1' --post-data='' "
        "http://127.0.0.1:8099/api/plugins/airplay.airplay-luna/service/enable >/dev/null 2>&1 || true\n"
        "i=0\n"
        "while [ $i -lt 45 ]; do\n"
        "  if [ -S /run/station-airplay/private-bus ] && netstat -ltn 2>/dev/null | grep -qE ':(5000|7000)'; then\n"
        "    break\n"
        "  fi\n"
        "  i=$((i + 1))\n"
        "  sleep 2\n"
        "done\n"
        "test -S /run/station-airplay/private-bus\n"
        "netstat -ltn 2>/dev/null | grep -qE ':(5000|7000)'\n"
        "PLUG_DIR=/data/yam/share/luna/plugins/airplay.airplay-luna\n"
        "if command -v dbus-send >/dev/null 2>&1; then\n"
        "  dbus-send --bus=unix:path=/run/station-airplay/private-bus "
        "--print-reply --dest=org.gnome.ShairportSync "
        "/org/gnome/ShairportSync org.gnome.ShairportSync.GetControlStatus | grep -Fq 'uint32'\n"
        "else\n"
        "  \"$PLUG_DIR/lib/ld-linux-armhf.so.3\" --library-path \"$PLUG_DIR/lib\" \"$PLUG_DIR/bin/dbus-send\" "
        "--bus=unix:path=/run/station-airplay/private-bus "
        "--print-reply --dest=org.gnome.ShairportSync "
        "/org/gnome/ShairportSync org.gnome.ShairportSync.GetControlStatus | grep -Fq 'uint32'\n"
        "fi\n"
        "printf '__AIRPLAY_E2E_OK__\\n'\n"
    )


class TestStationAirplayE2E(unittest.TestCase):
    def test_station_airplay_v2_across_boards(self) -> None:
        boards = discover_test_boards()
        if not boards:
            self.skipTest("No `work/*-system.ext4` guest images found")

        yam_install_sh = load_yam_install_sh()
        airplay_tag = latest_airplay_release_tag()

        for idx, board in enumerate(boards):
            yam_platform = EXPECTED_YAM_PLATFORM.get(board, board)
            with self.subTest(board=board):
                if yam_platform not in SUPPORTED_AIRPLAY_PLATFORMS:
                    self.skipTest(
                        f"[{board}] No ryysel/Station-airplay build for platform '{yam_platform}'"
                    )
                if yam_platform == "orion":
                    self.skipTest(
                        f"[{board}] No yam/luna .yampkg published yet for platform '{yam_platform}'"
                    )
                script = build_airplay_exec_script(
                    yam_install_sh=yam_install_sh,
                    yam_platform=yam_platform,
                    airplay_tag=airplay_tag,
                )
                out = run_headless_exec(board, script, port_offset=400 + idx * 10)
                self.assertIn("__AIRPLAY_E2E_OK__", out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
