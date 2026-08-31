#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <destination-directory>" >&2
    exit 2
fi

destination=$1
metadata_file=$(mktemp)
package_file=$(mktemp)

cleanup() {
    rm -f "$metadata_file" "$package_file"
}
trap cleanup EXIT HUP INT TERM

cargo metadata --locked --format-version 1 >"$metadata_file"
mkdir -p "$destination"

jq -r '.packages[] | [.name, .version, .manifest_path] | join("|")' "$metadata_file" >"$package_file"
while IFS='|' read -r package version manifest; do
    package_dir=$(dirname "$manifest")
    slug=$(printf '%s-%s' "$package" "$version" | tr '/:' '__')
    found=false
    for candidate in "$package_dir"/LICENSE* "$package_dir"/COPYING* "$package_dir"/NOTICE*; do
        if [ -f "$candidate" ]; then
            found=true
            filename=$(basename "$candidate")
            cp "$candidate" "$destination/$slug-$filename"
        fi
    done
    if [ "$found" = false ]; then
        printf '%s\n' "SPDX expression: $(jq -r --arg manifest "$manifest" '.packages[] | select(.manifest_path == $manifest) | .license' "$metadata_file")" >"$destination/$slug-SPDX.txt"
    fi
done <"$package_file"

cp LICENSE "$destination/Workdeck-MIT-LICENSE.txt"
