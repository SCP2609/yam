param(
    [string]$Version = $(if ($env:YAM_VERSION) { $env:YAM_VERSION } else { "latest" }),
    [string]$StationIp
)

$ErrorActionPreference = "Stop"
$repository = "https://github.com/nyakokitsu/yam"
$deviceRoot = if ($env:YAM_ROOT) { $env:YAM_ROOT } else { "/data/yam" }
if ($Version -eq "latest") {
    $releaseUrl = "$repository/releases/latest/download"
    $sourceRef = "v2"
} else {
    $tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
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
    Write-Host "YAM установлен: $deviceRoot/bin/yam"
    Write-Host "Пример: adb -s $serial shell '$deviceRoot/bin/yam --platform yandexmicro --profiles $deviceRoot/platforms list'"
} finally {
    if (Test-Path $tmp) { Remove-Item -Force -Recurse $tmp }
}
