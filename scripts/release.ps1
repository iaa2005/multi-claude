# Publishes a MultiClaude release on GitHub after scripts\build.cmd.
#
#   $env:GITHUB_TOKEN = "<token with contents:write on iaa2005/multi-claude>"
#   powershell -ExecutionPolicy Bypass -File scripts\release.ps1 -NotesFile notes.md
#
# The tag is v<version>-iaa2005 (the suffix keeps it apart from upstream CC
# Switch tags); the app version stays plain semver. The release carries the
# installer, the portable exe and latest.json, which installed copies read
# from releases/latest/download/latest.json to find and verify the update.
param(
  [Parameter(Mandatory = $true)][string]$NotesFile,
  [string]$Repo = "iaa2005/multi-claude"
)
$ErrorActionPreference = "Stop"
if (-not $env:GITHUB_TOKEN) { throw "Set GITHUB_TOKEN first" }

$root = Split-Path -Parent $PSScriptRoot
$version = (Get-Content "$root\package.json" -Raw | ConvertFrom-Json).version
$tag = "v$version-iaa2005"
$nsis = "$root\src-tauri\target\release\bundle\nsis"
$installerName = "MultiClaude_${version}_x64-setup.exe"
$installer = "$nsis\$installerName"
$signature = "$installer.sig"
# The binary is MultiClaude.exe since 4.0.14 (mainBinaryName); older builds left cc-switch.exe.
$portable = @("$root\src-tauri\target\release\MultiClaude.exe", "$root\src-tauri\target\release\cc-switch.exe") |
  Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $portable) { throw "Missing the portable exe in src-tauri\target\release - run scripts\build.cmd first" }
foreach ($file in $installer, $signature) {
  if (-not (Test-Path $file)) { throw "Missing $file - run scripts\build.cmd first" }
}

$notes = [IO.File]::ReadAllText((Resolve-Path $NotesFile), [Text.Encoding]::UTF8)
$downloadUrl = "https://github.com/$Repo/releases/download/$tag/$installerName"
$platform = @{ signature = (Get-Content $signature -Raw).Trim(); url = $downloadUrl }
$latest = [ordered]@{
  version   = $version
  notes     = "https://github.com/$Repo/releases/tag/$tag"
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{ "windows-x86_64" = $platform; "windows-x86_64-nsis" = $platform }
}
$latestPath = Join-Path $env:TEMP "multiclaude-latest.json"
[IO.File]::WriteAllText($latestPath, ($latest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))

$api = "https://api.github.com/repos/$Repo"
$headers = @{ Authorization = "Bearer $env:GITHUB_TOKEN"; Accept = "application/vnd.github+json" }
$body = @{ tag_name = $tag; target_commitish = "main"; name = "MultiClaude $version"; body = $notes; make_latest = "true" } | ConvertTo-Json
$release = Invoke-RestMethod -Method Post -Uri "$api/releases" -Headers $headers `
  -Body ([Text.Encoding]::UTF8.GetBytes($body)) -ContentType "application/json; charset=utf-8"

$upload = $release.upload_url -replace '\{.*\}$', ''
$assets = @(
  @{ path = $installer; name = $installerName },
  @{ path = $portable; name = "MultiClaude.exe" },
  @{ path = $latestPath; name = "latest.json" }
)
foreach ($asset in $assets) {
  Invoke-RestMethod -Method Post -Uri "${upload}?name=$($asset.name)" -Headers $headers `
    -ContentType "application/octet-stream" -InFile $asset.path | Out-Null
  Write-Host "uploaded $($asset.name)"
}
Write-Host "released $tag -> $($release.html_url)"
