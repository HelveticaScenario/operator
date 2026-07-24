# Windows counterpart of upload-release-assets.sh. Uploads the Squirrel.Windows
# artifacts (the RELEASES manifest, the nupkg it references, and Setup.exe) to
# the generic Package Registry and emits one asset-link JSON per file into
# assets/ for the release job. The RELEASES + nupkg pair is what the in-app
# updater reads from the release "permalink/latest/downloads" URL.
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path assets | Out-Null

$files = Get-ChildItem -Path out/make -Recurse -File | Where-Object {
    $_.Extension -in '.exe', '.nupkg' -or $_.Name -eq 'RELEASES'
}

foreach ($f in $files) {
    # Setup.exe contains a space; keep registry paths and filepaths URL-safe.
    $name = $f.Name -replace ' ', '.'
    $url = "$($env:CI_API_V4_URL)/projects/$($env:CI_PROJECT_ID)/packages/generic/operator/$($env:CI_COMMIT_TAG)/$name"
    Write-Host "Uploading $name"
    curl.exe --fail --silent --show-error --header "JOB-TOKEN: $($env:CI_JOB_TOKEN)" --upload-file $f.FullName $url
    $json = "{""name"":""$name"",""url"":""$url"",""filepath"":""/$name"",""link_type"":""package""}"
    Set-Content -Path "assets/$name.json" -Value $json -NoNewline
}
