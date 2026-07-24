#!/usr/bin/env bash
# Upload every release artifact under the given directories to the GitLab
# generic Package Registry and emit one asset-link JSON per file into assets/
# for the release job to attach. Each link carries a filepath so the file is
# reachable at the release "permalink/latest/downloads/<name>" URL that the
# app's in-app updater reads (see src/main/main.ts UPDATE_DOWNLOADS_URL).
set -euo pipefail

mkdir -p assets

for dir in "$@"; do
    [ -d "$dir" ] || continue
    while IFS= read -r -d '' f; do
        # Registry paths and download filepaths must be URL-safe; the Squirrel
        # "Setup.exe" installer name contains a space, so collapse whitespace.
        name="$(basename "$f" | tr ' ' '.')"
        url="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/packages/generic/operator/${CI_COMMIT_TAG}/${name}"
        echo "Uploading ${name}"
        curl --fail --silent --show-error \
            --header "JOB-TOKEN: ${CI_JOB_TOKEN}" \
            --upload-file "$f" "$url"
        printf '{"name":"%s","url":"%s","filepath":"/%s","link_type":"package"}' \
            "$name" "$url" "$name" >"assets/${name}.json"
    done < <(find "$dir" -type f \( \
        -name '*.zip' -o -name '*.dmg' -o -name '*.deb' -o -name '*.rpm' \
        -o -name '*.exe' -o -name '*.nupkg' -o -name 'RELEASES' \
        -o -name 'latest-mac.yml' \) -print0)
done
