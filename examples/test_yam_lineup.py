#!/usr/bin/env python3
"""
Пример E2E-теста установщика пакетного менеджера YAM v0.2+ (https://github.com/nyakokitsu/yam):
берёт оригинальный install.sh (из локального ../yam/install.sh при наличии либо из репозитория YAM),
вырезает из него без модификаций функции device_identity() и detect_platform() и сверяет результат
автоопределения с каноническим профилем платы по SoC и device-tree через
`kuyase run --headless --board <board> --snapshot --exec`.

Запуск:
  python examples/test_yam_lineup.py              # Все собранные платы в work/
  python examples/test_yam_lineup.py --board station2
  python examples/test_yam_lineup.py --ref main
"""

import argparse
import os
import pathlib
import platform
import re
import subprocess
import sys
import urllib.request

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
YAM_REF_DEFAULT = "main"


def load_yam_install_sh(ref: str = YAM_REF_DEFAULT) -> str:
    env_dir = os.environ.get("YAM_REPO_DIR", "").strip()
    local_candidates = []
    if env_dir:
        local_candidates.append(pathlib.Path(env_dir) / "install.sh")
    local_candidates.append(REPO_ROOT.parent / "yam" / "install.sh")

    if ref == YAM_REF_DEFAULT:
        for cand in local_candidates:
            if cand.is_file():
                print(f"[*] Используется локальный install.sh: {cand}")
                return cand.read_text(encoding="utf-8").replace("\r\n", "\n")

    url = f"https://raw.githubusercontent.com/nyakokitsu/yam/{ref}/install.sh"
    print(f"[*] Скачивание install.sh ({ref}): {url}")
    with urllib.request.urlopen(url, timeout=15) as resp:
        return resp.read().decode("utf-8").replace("\r\n", "\n")


def fetch_yam_detect_script(ref: str = YAM_REF_DEFAULT) -> str:
    install_sh = load_yam_install_sh(ref)
    print(
        "[*] Легенда проверки:\n"
        "    - compatible  : строка из /proc/device-tree/compatible (SoC + плата)\n"
        "    - quasar.cfg  : поле deviceType из /system/vendor/quasar/quasar.cfg\n"
        "    - expected    : эталонный профиль YAM v0.2+ / Luna (с учётом SoC AXG vs A5)\n"
        "    - yam_profile : профиль, который вернула функция detect_platform() из install.sh\n",
        flush=True,
    )

    id_match = re.search(r"(device_identity\(\)\s*\{.*?\n\})", install_sh, re.DOTALL)
    det_match = re.search(r"(detect_platform\(\)\s*\{.*?\n\})", install_sh, re.DOTALL)
    if not id_match or not det_match:
        sys.exit("[ERROR] В install.sh не найдены функции device_identity() / detect_platform()")

    return (
        'adb() { shift 3; sh -c "$1"; }\n'
        "DEVICE=local\n"
        f"{id_match.group(1)}\n"
        f"{det_match.group(1)}\n"
        "DEVICE_IDENTITY=$(device_identity)\n"
        'QUASAR_CFG=$(sed -n \'s/.*"deviceType"[[:space:]]*:[[:space:]]*"\\([^"]*\\)".*/\\1/p\' /system/vendor/quasar/quasar.cfg 2>/dev/null | head -n 1)\n'
        'DT_RAW=$(tr "\\000" " " </proc/device-tree/compatible 2>/dev/null)\n'
        'case "$QUASAR_BOARD" in\n'
        '  station2)                  EXPECTED="yandexstation_2" ;;\n'
        '  cucumber)                  EXPECTED="cucumber" ;;\n'
        '  yandexmini2|yandexmini2nc) EXPECTED="yandexmini_2" ;;\n'
        '  plum|plumnc)               EXPECTED="plum" ;;\n'
        '  *)                         EXPECTED="$QUASAR_BOARD" ;;\n'
        "esac\n"
        'printf "  compatible  : %s\\n" "$DT_RAW"\n'
        'printf "  quasar.cfg  : %s\\n" "${QUASAR_CFG:-unknown}"\n'
        'printf "  expected    : %s\\n" "$EXPECTED"\n'
        "if ! plat=$(detect_platform); then\n"
        '  printf "  yam_profile : НЕ РАСПОЗНАНА (detect_platform вернул ошибку)\\n"\n'
        "  exit 1\n"
        "fi\n"
        'printf "  yam_profile : %s\\n" "$plat"\n'
        'if [ "$plat" != "$EXPECTED" ]; then\n'
        '  printf "  ОШИБКА      : по SoC/DT ожидался профиль \'%s\', но detect_platform() вернул \'%s\' (quasar.cfg=\'%s\')\\n" "$EXPECTED" "$plat" "$QUASAR_CFG"\n'
        "  exit 1\n"
        "fi"
    )


def find_kuyase() -> str:
    exe = "kuyase.exe" if platform.system() == "Windows" else "kuyase"
    for p in (
        REPO_ROOT / "target" / "release" / exe,
        REPO_ROOT / "Releases" / exe,
        REPO_ROOT / exe,
    ):
        if p.is_file():
            return str(p)
    return exe


def main() -> None:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    parser = argparse.ArgumentParser(description="Проверка detect_platform() из YAM в KUYASE")
    parser.add_argument("--board", default="all", help="Код платы (например, station2) или all")
    parser.add_argument("--ref", default=YAM_REF_DEFAULT, help="Коммит или ветка YAM (по умолчанию main)")
    args = parser.parse_args()

    guest_cmd = fetch_yam_detect_script(args.ref)
    cmd = [
        find_kuyase(),
        "run",
        "--headless",
        "--board",
        args.board,
        "--snapshot",
        "--exec",
        guest_cmd,
    ]
    sys.exit(subprocess.call(cmd, cwd=REPO_ROOT))


if __name__ == "__main__":
    main()
