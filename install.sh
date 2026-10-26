#!/bin/sh
set -eu

VERSION=${1:-${YAM_VERSION:-latest}}
REPOSITORY=https://github.com/nyakokitsu/yam
DEVICE_ROOT=${YAM_ROOT:-/data/yam}

case "$VERSION" in
  latest) RELEASE_URL="$REPOSITORY/releases/latest/download"; SOURCE_REF=v2 ;;
  v*) RELEASE_URL="$REPOSITORY/releases/download/$VERSION"; SOURCE_REF="$VERSION" ;;
  *) RELEASE_URL="$REPOSITORY/releases/download/v$VERSION"; SOURCE_REF="v$VERSION" ;;
esac

TMPDIR_INSTALL=$(mktemp -d "${TMPDIR:-/tmp}/yam.XXXXXX")
trap 'rm -rf "$TMPDIR_INSTALL"' EXIT HUP INT TERM

download() {
  url=$1 output=$2
  if command -v curl >/dev/null 2>&1; then
    curl -fL --proto '=https,http' --tlsv1.2 "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$output" "$url"
  else
    echo 'Нужен curl или wget.' >&2
    exit 4
  fi
}

verify() {
  file=$1
  expected=$(awk -v name="$(basename "$file")" '$2 == name {print $1}' "$TMPDIR_INSTALL/SHA256SUMS")
  [ -n "$expected" ] || { echo "Нет контрольной суммы для $(basename "$file")." >&2; exit 5; }
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$file" | awk '{print $1}')
  elif command -v openssl >/dev/null 2>&1; then
    actual=$(openssl dgst -sha256 "$file" | awk '{print $NF}')
  else
    echo 'Нужен sha256sum или openssl.' >&2
    exit 5
  fi
  [ "$actual" = "$expected" ] || { echo 'Контрольная сумма не совпадает.' >&2; exit 5; }
}

command -v adb >/dev/null 2>&1 || { echo 'Нужен adb из Android Platform Tools.' >&2; exit 4; }

choose_device() {
  devices=$(adb devices </dev/null | awk 'NR > 1 && $2 == "device" {print $1}')
  [ -n "$devices" ] || return 1
  count=$(printf '%s\n' "$devices" | wc -l | tr -d ' ')
  if [ "$count" -eq 1 ]; then printf '%s\n' "$devices"; return 0; fi
  echo 'Подключённые колонки:' >&2
  printf '%s\n' "$devices" | nl -ba >&2
  printf 'Номер: ' >&2
  IFS= read -r selection </dev/tty
  case "$selection" in ''|*[!0-9]*) return 1 ;; esac
  printf '%s\n' "$devices" | sed -n "${selection}p"
}

DEVICE=$(choose_device || true)
if [ -z "$DEVICE" ]; then
  printf 'IP-адрес колонки: ' >&2
  IFS= read -r ip </dev/tty
  [ -n "$ip" ] || { echo 'Нужен IP-адрес.' >&2; exit 6; }
  DEVICE="$ip:5555"
  adb connect "$DEVICE" </dev/null >&2
  adb -s "$DEVICE" get-state </dev/null >/dev/null
fi

ASSET=yam-aarch64-unknown-linux-musl
download "$RELEASE_URL/SHA256SUMS" "$TMPDIR_INSTALL/SHA256SUMS"
download "$RELEASE_URL/$ASSET" "$TMPDIR_INSTALL/$ASSET"
verify "$TMPDIR_INSTALL/$ASSET"

adb -s "$DEVICE" shell "mkdir -p '$DEVICE_ROOT/bin' '$DEVICE_ROOT/platforms'" </dev/null
adb -s "$DEVICE" push "$TMPDIR_INSTALL/$ASSET" "$DEVICE_ROOT/bin/yam.new" </dev/null >/dev/null
for profile in yandexmicro yandexmini_2 yandexmidi yandexplum; do
  download "$REPOSITORY/raw/$SOURCE_REF/platforms/$profile.yaml" "$TMPDIR_INSTALL/$profile.yaml"
  adb -s "$DEVICE" push "$TMPDIR_INSTALL/$profile.yaml" "$DEVICE_ROOT/platforms/$profile.yaml" </dev/null >/dev/null
done
adb -s "$DEVICE" shell "chmod 0755 '$DEVICE_ROOT/bin/yam.new'; mv '$DEVICE_ROOT/bin/yam.new' '$DEVICE_ROOT/bin/yam'" </dev/null

echo "YAM установлен на $DEVICE: $DEVICE_ROOT/bin/yam"
echo "Пример: adb -s $DEVICE shell '$DEVICE_ROOT/bin/yam --platform yandexmicro --profiles $DEVICE_ROOT/platforms list'"
