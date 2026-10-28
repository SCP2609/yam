#!/bin/sh
set -eu

VERSION=${YAM_VERSION:-latest}
REMOVE_OLD=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --remove-old) REMOVE_OLD=1 ;;
    --help|-h)
      echo 'Использование: install.sh [версия] [--remove-old]'
      echo '  --remove-old  удалить legacy yamd из /data/quazar/yam'
      exit 0
      ;;
    -*) echo "Неизвестный флаг: $1" >&2; exit 2 ;;
    *) VERSION=$1 ;;
  esac
  shift
done
REPOSITORY=https://github.com/nyakokitsu/yam
DEVICE_ROOT=${YAM_ROOT:-/data/yam}
PLATFORM=${YAM_PLATFORM:-}

if [ "$REMOVE_OLD" -eq 1 ] && [ "$DEVICE_ROOT" = /data/quazar/yam ]; then
  echo 'YAM_ROOT не может указывать на legacy-каталог при --remove-old.' >&2
  exit 2
fi

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

latest_tag() {
  api="https://api.github.com/repos/nyakokitsu/yam/releases?per_page=100"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --proto '=https' --tlsv1.2 "$api"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "$api"
  else
    return 1
  fi | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\(yam-v[^"]*\)".*/\1/p' | head -n 1
}

case "$VERSION" in
  latest) TAG=$(latest_tag); [ -n "$TAG" ] || { echo 'Не найден релиз канала yam-v.' >&2; exit 4; }; SOURCE_REF=main ;;
  yam-v*) TAG="$VERSION"; SOURCE_REF="$VERSION" ;;
  *) TAG="yam-v$VERSION"; SOURCE_REF="$TAG" ;;
esac
RELEASE_URL="$REPOSITORY/releases/download/$TAG"

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

device_identity() {
  adb -s "$DEVICE" shell 'getprop ro.product.model; getprop ro.product.device; cat /proc/device-tree/compatible 2>/dev/null | tr "\000" "\n"' </dev/null 2>/dev/null \
    | tr -d '\r' | tr '[:upper:]' '[:lower:]'
}

detect_platform() {
  case "$DEVICE_IDENTITY" in
    *yandex,station-mini2*|*station-mini2*) printf '%s\n' yandexmini_2 ;;
    *yandex,cucumber-dvt2*|*cucumber*) printf '%s\n' cucumber ;;
    *yandex,station-midi*) printf '%s\n' yandexmidi ;;
    *yandex,orion-pvt*|*orion*) printf '%s\n' orion ;;
    *yandex,pickle-evt*|*pickle*) printf '%s\n' pickle ;;
    *yandex,station-mini3*|*bergamot*) printf '%s\n' bergamot ;;
    *yandex,saturn*|*saturn*) printf '%s\n' saturn ;;
    *tulip-d1*|*tulip*) printf '%s\n' yandexstation ;;
    *yandexstation_2*|*station-max*) printf '%s\n' yandexstation_2 ;;
    *mediatek,mt8167*|*jbl*link*music*) printf '%s\n' jbl_link_music ;;
    *mango*) printf '%s\n' mango ;;
    *plum*) printf '%s\n' plum ;;
    *yandex*station*micro*|*yandexmicro*) printf '%s\n' yandexmicro ;;
    *amlogic,axg*) printf '%s\n' yandexmini ;;
    *) return 1 ;;
  esac
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

if [ -z "$PLATFORM" ]; then
  DEVICE_IDENTITY=$(device_identity)
  PLATFORM=$(detect_platform || true)
  if [ -z "$PLATFORM" ]; then
    echo 'Не удалось определить платформу колонки. Задайте YAM_PLATFORM явно.' >&2
    printf '%s\n' "$DEVICE_IDENTITY" >&2
    exit 3
  fi
  echo "Определена платформа: $PLATFORM" >&2
fi

case "$PLATFORM" in
  mango|jbl_link_music) ASSET=yam-armv7-unknown-linux-musleabihf ;;
  *) ASSET=yam-aarch64-unknown-linux-musl ;;
esac

if [ "$REMOVE_OLD" -eq 1 ]; then
  adb -s "$DEVICE" shell 'set -eu
    legacy=/data/quazar/yam
    if [ -r "$legacy/yamd.pid" ]; then
      pid=$(cat "$legacy/yamd.pid" 2>/dev/null || true)
      case "$pid" in ""|*[!0-9]*) ;; *) kill "$pid" 2>/dev/null || true ;; esac
    fi
    mount -o remount,rw /
    trap "mount -o remount,ro /" EXIT
    if [ -f /etc/init.d/S99yam ] && grep -q "/data/quazar/yam" /etc/init.d/S99yam; then
      rm -f /etc/init.d/S99yam
    fi
    rm -rf "$legacy"' </dev/null
  echo 'Удалены legacy yamd и его данные из /data/quazar/yam.' >&2
fi

download "$RELEASE_URL/SHA256SUMS" "$TMPDIR_INSTALL/SHA256SUMS"
download "$RELEASE_URL/$ASSET" "$TMPDIR_INSTALL/$ASSET"
verify "$TMPDIR_INSTALL/$ASSET"

adb -s "$DEVICE" shell "mkdir -p '$DEVICE_ROOT/bin' '$DEVICE_ROOT/platforms'" </dev/null
adb -s "$DEVICE" push "$TMPDIR_INSTALL/$ASSET" "$DEVICE_ROOT/bin/yam.new" </dev/null >/dev/null
for profile in yandexmicro plum yandexplum yandexmini yandexmini_2 bergamot pickle cucumber yandexmidi mango yandexstation_2 yandexstation saturn jbl_link_music orion; do
  download "$REPOSITORY/raw/$SOURCE_REF/platforms/$profile.yaml" "$TMPDIR_INSTALL/$profile.yaml"
  adb -s "$DEVICE" push "$TMPDIR_INSTALL/$profile.yaml" "$DEVICE_ROOT/platforms/$profile.yaml" </dev/null >/dev/null
done
adb -s "$DEVICE" shell "chmod 0755 '$DEVICE_ROOT/bin/yam.new'; mv '$DEVICE_ROOT/bin/yam.new' '$DEVICE_ROOT/bin/yam'" </dev/null
cat >"$TMPDIR_INSTALL/yam-run" <<EOF
#!/bin/sh
export YAM_PLATFORM='$PLATFORM'
export YAM_PROFILES='$DEVICE_ROOT/platforms'
exec '$DEVICE_ROOT/bin/yam' "\$@"
EOF
adb -s "$DEVICE" push "$TMPDIR_INSTALL/yam-run" "$DEVICE_ROOT/bin/yam-run.new" </dev/null >/dev/null
adb -s "$DEVICE" shell "chmod 0755 '$DEVICE_ROOT/bin/yam-run.new'; mv '$DEVICE_ROOT/bin/yam-run.new' '$DEVICE_ROOT/bin/yam-run'; test -s '$DEVICE_ROOT/bin/yam-run'" </dev/null

if [ "$PLATFORM" = yandexmicro ]; then
  adb -s "$DEVICE" shell "set -eu
    mount -o remount,rw /
    trap 'mount -o remount,ro /' EXIT
    if [ -e /usr/bin/yam ] && { [ ! -L /usr/bin/yam ] || [ \"\$(readlink /usr/bin/yam)\" != '$DEVICE_ROOT/bin/yam-run' ]; }; then
      echo '/usr/bin/yam already belongs to another program' >&2; exit 1
    fi
    ln -sfn '$DEVICE_ROOT/bin/yam-run' /usr/bin/yam
    cat >/etc/init.d/S99yam <<'HOOK'
#!/bin/sh
root='$DEVICE_ROOT'
platform='$PLATFORM'
case \"\${1:-}\" in
  start)
    [ -x \"\$root/bin/yam\" ] || exit 0
    ln -sfn "\$root/bin/yam-run" /usr/bin/yam
    /usr/bin/yam update || true
    ;;
esac
HOOK
    chmod 0755 /etc/init.d/S99yam" </dev/null
else
  echo "Автообновление при загрузке настроено только для yandexmicro; используйте: yam --platform $PLATFORM --profiles $DEVICE_ROOT/platforms update" >&2
fi

echo "YAM установлен на $DEVICE: $DEVICE_ROOT/bin/yam"
echo "Пример: adb -s $DEVICE shell 'yam list'"
