param(
    [string]$Version = $(if ($env:YAM_VERSION) { $env:YAM_VERSION } else { "latest" }),
    [string]$StationIp,
    [string]$Platform = $(if ($env:YAM_PLATFORM) { $env:YAM_PLATFORM } else { "yandexmicro" }),
    [switch]$RemoveOld
)

$ErrorActionPreference = "Stop"
$repository = "https://github.com/nyakokitsu/yam"
$deviceRoot = if ($env:YAM_ROOT) { $env:YAM_ROOT } else { "/data/yam" }
if ($Version -eq "latest") {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/nyakokitsu/yam/releases?per_page=100" | Where-Object { -not $_.draft -and -not $_.prerelease -and $_.tag_name -like "yam-v*" } | Select-Object -First 1
    if (-not $release) { throw "Не найден релиз канала yam-v." }
    $tag = $release.tag_name
    $releaseUrl = "$repository/releases/download/$tag"
    $sourceRef = "main"
} else {
    $tag = if ($Version.StartsWith("yam-v")) { $Version } else { "yam-v$Version" }
    $releaseUrl = "$repository/releases/download/$tag"
    $sourceRef = $tag
}

$adb = Get-Command adb -ErrorAction SilentlyContinue
if (-not $adb) { throw "Нужен adb из Android Platform Tools." }

function Get-Device {
    $devices = @(& adb devices | Select-Object -Skip 1 | ForEach-Object {
        $parts = $_ -split "`t"
        if ($parts.Count -ge 2 -and $parts[1] -eq "device") { $parts[0] }
    }) | Where-Object { $_ }
    if ($devices.Count -eq 1) { return $devices[0] }
    if ($devices.Count -gt 1) {
        for ($i = 0; $i -lt $devices.Count; $i++) { Write-Host "$($i + 1)) $($devices[$i])" }
        $choice = [int](Read-Host "Номер колонки") - 1
        if ($choice -ge 0 -and $choice -lt $devices.Count) { return $devices[$choice] }
        throw "Неверный номер колонки."
    }
    if (-not $StationIp) { $StationIp = Read-Host "IP-адрес колонки" }
    if (-not $StationIp) { throw "Нужен IP-адрес колонки." }
    $serial = "$StationIp`:5555"
    & adb connect $serial | Out-Host
    & adb -s $serial get-state | Out-Null
    return $serial
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("yam-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $serial = Get-Device
    if ($RemoveOld) {
        if ($deviceRoot -eq "/data/quazar/yam") { throw "YAM_ROOT не может указывать на legacy-каталог с -RemoveOld." }
        $legacyCleanup = @'
set -eu
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
rm -rf "$legacy"
'@
        & adb -s $serial shell $legacyCleanup | Out-Null
        Write-Host "Удалены legacy yamd и его данные из /data/quazar/yam."
    }
    $asset = "yam-aarch64-unknown-linux-musl"
    $checksums = Join-Path $tmp "SHA256SUMS"
    $binary = Join-Path $tmp $asset
    Invoke-WebRequest -Uri "$releaseUrl/SHA256SUMS" -OutFile $checksums
    Invoke-WebRequest -Uri "$releaseUrl/$asset" -OutFile $binary
    $expected = ((Get-Content $checksums | Where-Object { $_ -match ("\s" + [regex]::Escape($asset) + "$") }) -split "\s+")[0]
    if (-not $expected -or (Get-FileHash -Algorithm SHA256 $binary).Hash.ToLowerInvariant() -ne $expected.ToLowerInvariant()) { throw "Контрольная сумма не совпадает." }

    & adb -s $serial shell "mkdir -p '$deviceRoot/bin' '$deviceRoot/platforms'" | Out-Null
    & adb -s $serial push $binary "$deviceRoot/bin/yam.new" | Out-Null
    foreach ($profile in @("yandexmicro", "yandexmini_2", "yandexmidi", "yandexplum")) {
        $profileFile = Join-Path $tmp "$profile.yaml"
        Invoke-WebRequest -Uri "$repository/raw/$sourceRef/platforms/$profile.yaml" -OutFile $profileFile
        & adb -s $serial push $profileFile "$deviceRoot/platforms/$profile.yaml" | Out-Null
    }
    & adb -s $serial shell "chmod 0755 '$deviceRoot/bin/yam.new'; mv '$deviceRoot/bin/yam.new' '$deviceRoot/bin/yam'" | Out-Null
    $runner = @"
#!/bin/sh
export YAM_PLATFORM='$Platform'
export YAM_PROFILES='$deviceRoot/platforms'
exec '$deviceRoot/bin/yam' "`$@"
"@
    $runner64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($runner))
    & adb -s $serial shell "echo '$runner64' | base64 -d > '$deviceRoot/bin/yam-run'; chmod 0755 '$deviceRoot/bin/yam-run'" | Out-Null
    if ($Platform -eq "yandexmicro") {
        $bootScript = @"
#!/bin/sh
root='$deviceRoot'
platform='$Platform'
case "`${1:-}" in
  start)
    [ -x "`$root/bin/yam" ] || exit 0
    ln -sfn "`$root/bin/yam-run" /usr/bin/yam
    /usr/bin/yam update || true
    ;;
esac
"@
        $bootScript64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($bootScript))
        $bootCommand = "set -eu; mount -o remount,rw /; trap 'mount -o remount,ro /' EXIT; if [ -e /usr/bin/yam ] && { [ ! -L /usr/bin/yam ] || [ `$(readlink /usr/bin/yam) != '$deviceRoot/bin/yam-run' ]; }; then exit 1; fi; ln -sfn '$deviceRoot/bin/yam-run' /usr/bin/yam; echo '$bootScript64' | base64 -d > /etc/init.d/S99yam; chmod 0755 /etc/init.d/S99yam"
        & adb -s $serial shell $bootCommand | Out-Null
    } else {
        Write-Warning "Автообновление при загрузке настроено только для yandexmicro."
    }
    Write-Host "YAM установлен: $deviceRoot/bin/yam"
    Write-Host "Пример: adb -s $serial shell 'yam list'"
} finally {
    if (Test-Path $tmp) { Remove-Item -Force -Recurse $tmp }
}
