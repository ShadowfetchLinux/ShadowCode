#!/usr/bin/env bash
# Build-only container: no interpreter or compiler is copied into the application.
set -euo pipefail
source /build/fetch-source.sh
mkdir -p /out/sources /out/notices /build/dependencies
UPSTREAM_SHA="$(jq -r .upstream.sha256 manifest.json)"
printf '%s  upstream.tar.gz\n' "$UPSTREAM_SHA" | sha256sum -c -
tar -xzf upstream.tar.gz
UPSTREAM_COMMIT="$(jq -r .upstream.commit manifest.json)"
RUNTIME_SOURCE="/build/type2-runtime-$UPSTREAM_COMMIT"
patch --batch --fuzz=0 -d "$RUNTIME_SOURCE" -p1 < isolated-extraction.patch
cp upstream.tar.gz manifest.json alpine-sources.json isolated-extraction.patch build.sh fetch-source.sh Dockerfile apk-packages.txt /out/sources/
cp "$RUNTIME_SOURCE/LICENSE" /out/notices/type2-runtime-LICENSE

while IFS=$'\t' read -r archive url digest; do
    fetch_source "/out/sources/$archive" sha256 "$digest" "/build/$archive" "$url"
    tar -xf "/out/sources/$archive" -C /build/dependencies
done < <(jq -r '.sources[] | [.archive,.url,.sha256] | @tsv' manifest.json)

cd /build/dependencies/fuse-3.15.0
patch --batch --fuzz=0 -p1 < "$RUNTIME_SOURCE/patches/libfuse/mount.c.diff"
meson setup --prefix=/usr --default-library=static build
ninja -C build install
cp LGPL2.txt /out/notices/libfuse-LGPL2.txt
cp GPL2.txt /out/notices/libfuse-GPL2.txt
cp LICENSE /out/notices/libfuse-LICENSE

cd /build/dependencies/squashfuse-0.5.2
export CFLAGS='-ffunction-sections -fdata-sections -Os'
./autogen.sh
./configure LDFLAGS=-static
make -j"$(nproc)"
make install
install -m 644 ./*.h /usr/local/include/squashfuse
cp LICENSE /out/notices/squashfuse-LICENSE

cd /build
# Retain the exact sources, distribution patches, and license texts for the
# Alpine libraries linked into the runtime. These are separate source artifacts;
# only the original notices and provenance are copied into the application.
while IFS=$'\t' read -r name archive directory; do
    mkdir -p "/out/sources/alpine/$name" "/out/notices/$name"
    while IFS=$'\t' read -r file digest urls; do
        IFS=$'\t' read -r -a candidates <<< "$urls"
        fetch_source "/out/sources/alpine/$name/$file" sha512 "$digest" "/build/alpine/$name/$file" "${candidates[@]}"
    done < <(jq -r --arg name "$name" '.packages[] | select(.name == $name) | .files[] | [.file,.sha512,.url,(.mirrors // [])[]] | @tsv' alpine-sources.json)
    while IFS= read -r notice; do
        tar -xOf "/out/sources/alpine/$name/$archive" "$directory/$notice" > "/out/notices/$name/$notice"
        test -s "/out/notices/$name/$notice"
    done < <(jq -r --arg name "$name" '.packages[] | select(.name == $name) | .notices[]' alpine-sources.json)
done < <(jq -r '.packages[] | [.name,.archive,.sourceDirectory] | @tsv' alpine-sources.json)

cd "$RUNTIME_SOURCE"
printf 'https://github.com/AppImage/type2-runtime/commit/%s\n' "$UPSTREAM_COMMIT" > src/runtime/version
sed -i 's/-static-pie/-static-pie -Wl,-Map,\/out\/runtime.map/' src/runtime/Makefile
bash scripts/build-runtime.sh
cp out/runtime-x86_64 out/runtime-x86_64.debug /out/
cp /lib/apk/db/installed /out/build-packages.db
apk info -vv | sort > /out/build-packages.txt
clang --version > /out/compiler.txt
readelf -d /out/runtime-x86_64 > /out/runtime-dynamic.txt
if grep -q NEEDED /out/runtime-dynamic.txt; then
    echo 'The AppImage runtime must link statically.' >&2
    exit 1
fi
/out/runtime-x86_64 --appimage-version
