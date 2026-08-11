#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
data_dir=${SHOWCASE_DATA_DIR:-"$root_dir/target/demo-data"}
image_dir="$data_dir/image-registration"
cloud_dir="$data_dir/point-cloud-registration"

image_url=${SHOWCASE_IMAGE_URL:-"https://raw.githubusercontent.com/opencv/opencv/77dfa297d08fdecdc509fc01ad92a2e9ec776a57/samples/data/building.jpg"}
image_sha=${SHOWCASE_IMAGE_SHA256:-"742a1baad62ac82e91e718e77eedf7e85c2eddc4badfb8c87c6cbc86c45a8b07"}
cloud_url=${SHOWCASE_CLOUD_URL:-"https://github.com/isl-org/open3d_downloads/releases/download/20220201-data/DemoICPPointClouds.zip"}
cloud_sha=${SHOWCASE_CLOUD_SHA256:-"7596ffc80afe992ed966f4d96b676a08d9393fd86ed8bfd672b2f6a514c6fb75"}

mkdir -p "$image_dir" "$cloud_dir"
temp_dir=$(mktemp -d "$data_dir/.showcase-fetch.XXXXXX")
trap 'rm -rf "$temp_dir"' EXIT

fetch_verified() {
    local url=$1
    local sha=$2
    local destination=$3
    local temp_file=$4

    if [[ -f "$destination" ]] && printf '%s  %s\n' "$sha" "$destination" | sha256sum --check --status; then
        return
    fi
    curl --fail --location --retry 3 --output "$temp_file" "$url"
    printf '%s  %s\n' "$sha" "$temp_file" | sha256sum --check --status || {
        echo "checksum verification failed for $url" >&2
        exit 1
    }
    mv "$temp_file" "$destination"
}

fetch_verified "$image_url" "$image_sha" "$image_dir/building.jpg" "$temp_dir/building.jpg"
fetch_verified "$cloud_url" "$cloud_sha" "$cloud_dir/DemoICPPointClouds.zip" "$temp_dir/DemoICPPointClouds.zip"

extract_dir="$temp_dir/clouds"
mkdir -p "$extract_dir"
unzip -qq "$cloud_dir/DemoICPPointClouds.zip" 'cloud_bin_0.pcd' 'cloud_bin_1.pcd' 'init.log' -d "$extract_dir"
for file in cloud_bin_0.pcd cloud_bin_1.pcd init.log; do
    cmp -s "$extract_dir/$file" "$cloud_dir/$file" 2>/dev/null || mv "$extract_dir/$file" "$cloud_dir/$file"
done

echo "showcase data ready at target/demo-data"
