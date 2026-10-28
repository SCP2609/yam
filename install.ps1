param(
    [string]$Version = $(if ($env:YAM_VERSION) { $env:YAM_VERSION } else { "latest" }),
    [string]$StationIp,
    [string]$Platform = $env:YAM_PLATFORM,
    [switch]$RemoveOld
)

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$repository = "https://github.com/nyakokitsu/yam"
$deviceRoot = if ($env:YAM_ROOT) { $env:YAM_ROOT } else { "/data/yam" }
if ($Version -eq "latest") {
    $apiHeaders = @{ "User-Agent" = "YAM-installer"; "Accept" = "application/vnd.github+json" }
    $releases = @(Invoke-RestMethod -Headers $apiHeaders -Uri "https://api.github.com/repos/nyakokitsu/yam/releases?per_page=100")
    $release = @($releases | Where-Object {
            ($_.draft -eq $false) -and
            ($_.prerelease -eq $false) -and
            ($_.tag_name -like "yam-v*")
        })[0]
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

function Download-File([string]$Uri, [string]$OutFile) {
    $parameters = @{ Uri = $Uri; OutFile = $OutFile; ErrorAction = "Stop" }
    if ($PSVersionTable.PSVersion.Major -lt 6) { $parameters.UseBasicParsing = $true }
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Invoke-WebRequest @parameters
            return
        } catch {
            if ($attempt -eq 3) { throw }
            Start-Sleep -Seconds $attempt
        }
    }
}

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

function Get-DetectedPlatform([string]$Serial) {
    $identity = (& adb -s $Serial shell 'getprop ro.product.model; getprop ro.product.device; cat /proc/device-tree/compatible 2>/dev/null | tr "\000" "\n"' 2>$null | ForEach-Object { $_.Trim().ToLowerInvariant() }) -join "`n"
    $platform = if ($identity -match 'yandex,station-mini2|station-mini2') { "yandexmini_2" }
    elseif ($identity -match 'yandex,cucumber-dvt2|cucumber') { "cucumber" }
    elseif ($identity -match 'yandex,station-midi') { "yandexmidi" }
    elseif ($identity -match 'yandex,orion-pvt|orion') { "orion" }
    elseif ($identity -match 'yandex,pickle-evt|pickle') { "pickle" }
    elseif ($identity -match 'yandex,station-mini3|bergamot') { "bergamot" }
    elseif ($identity -match 'yandex,saturn|saturn') { "saturn" }
    elseif ($identity -match 'tulip-d1|tulip') { "yandexstation" }
    elseif ($identity -match 'yandexstation_2|station-max') { "yandexstation_2" }
    elseif ($identity -match 'mediatek,mt8167|jbl.*link.*music') { "jbl_link_music" }
    elseif ($identity -match 'mango') { "mango" }
    elseif ($identity -match 'plum') { "plum" }
    elseif ($identity -match 'yandex.*station.*micro|yandexmicro') { "yandexmicro" }
    elseif ($identity -match 'amlogic,axg') { "yandexmini" }
    else { $null }
    if (-not $platform) { throw "Не удалось определить платформу колонки. Задайте -Platform явно.`n$identity" }
    return $platform
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("yam-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $serial = Get-Device
    if ([string]::IsNullOrWhiteSpace($Platform)) {
        $Platform = Get-DetectedPlatform $serial
        Write-Host "Определена платформа: $Platform"
    }
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
    $asset = if ($Platform -in @("mango", "jbl_link_music")) { "yam-armv7-unknown-linux-musleabihf" } else { "yam-aarch64-unknown-linux-musl" }
    $checksums = Join-Path $tmp "SHA256SUMS"
    $binary = Join-Path $tmp $asset
    Download-File "$releaseUrl/SHA256SUMS" $checksums
    Download-File "$releaseUrl/$asset" $binary
    $expected = ((Get-Content $checksums | Where-Object { $_ -match ("\s" + [regex]::Escape($asset) + "$") }) -split "\s+")[0]
    if (-not $expected -or (Get-FileHash -Algorithm SHA256 $binary).Hash.ToLowerInvariant() -ne $expected.ToLowerInvariant()) { throw "Контрольная сумма не совпадает." }

    & adb -s $serial shell "mkdir -p '$deviceRoot/bin' '$deviceRoot/platforms'" | Out-Null
    & adb -s $serial push $binary "$deviceRoot/bin/yam.new" | Out-Null
    foreach ($profile in @("yandexmicro", "plum", "yandexplum", "yandexmini", "yandexmini_2", "bergamot", "pickle", "cucumber", "yandexmidi", "mango", "yandexstation_2", "yandexstation", "saturn", "jbl_link_music", "orion")) {
        $profileFile = Join-Path $tmp "$profile.yaml"
        Download-File "$repository/raw/$sourceRef/platforms/$profile.yaml" $profileFile
        & adb -s $serial push $profileFile "$deviceRoot/platforms/$profile.yaml" | Out-Null
    }
    & adb -s $serial shell "chmod 0755 '$deviceRoot/bin/yam.new'; mv '$deviceRoot/bin/yam.new' '$deviceRoot/bin/yam'" | Out-Null
    $runner = @"
#!/bin/sh
export YAM_PLATFORM='$Platform'
export YAM_PROFILES='$deviceRoot/platforms'
exec '$deviceRoot/bin/yam' "`$@"
"@
    $runnerPath = Join-Path $tmp "yam-run"
    [IO.File]::WriteAllText($runnerPath, $runner, [Text.Encoding]::ASCII)
    & adb -s $serial push $runnerPath "$deviceRoot/bin/yam-run.new" | Out-Null
    & adb -s $serial shell "chmod 0755 '$deviceRoot/bin/yam-run.new'; mv '$deviceRoot/bin/yam-run.new' '$deviceRoot/bin/yam-run'; test -s '$deviceRoot/bin/yam-run'" | Out-Null
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
