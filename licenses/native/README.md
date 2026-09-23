# Native dependency notice sources

These are unmodified upstream license and copyright texts, retained when the
published crate omits them, plus notices for the Rust standard library and the
AppImage helpers. `sources.json` records their exact source commits or archives
and SHA-256 digests. Crate overrides are specific to the resolved version;
changing a dependency with no notice text fails packaging until its attribution
is supplied. The selectors crate declares MPL-2.0 in its source headers; its
complete license text comes from Mozilla.

The Rust library notices come from the official 1.95.0 compiler distribution,
whose archive digest was checked against the distribution's checksum. Changing
the compiler requires updating these standard-library notices. The AppImage
runtime is also checked during packaging before its notices are used.

`scripts/native-notices.mjs` collects notices from the installed, locked Cargo
and npm dependencies. The Cargo inventory includes the target's resolved build
and test dependencies, and the npm inventory includes all production packages.
Both inventories deliberately include some code eliminated during bundling.
They should not be interpreted as a list of functions present in the executable.

After linuxdeploy gathers the AppImage, the collector matches its libraries and
data files to the build host's package database, retains the distro copyright
files and common license texts, and records exact binary and source versions.
Unattributed system files stop packaging. The resulting application and system
inventories live under `usr/share/doc/shadowcode/notices` inside the AppImage.
The Debian package contains the application notices and uses system libraries.
The package checker verifies every listed notice's digest after extraction.

This inventory is part of release preparation. Corresponding source archives
for redistributed components and the remaining product verification gates must
also be addressed before the supported native release is published.

The final AppImage runtime is built from pinned source with ShadowCode's
[private-extraction patch and container recipe](../../packaging/native-runtime/README.md).
Its static dependency notices and build provenance live in the `runtime/`
subdirectory of the packaged notices. The adjacent runtime source archive
contains exact upstream sources and distribution patches; the package checker
verifies its hashes and links it to the runtime's compiled code. This supplements
the application/system inventories; it does not replace their release source
requirements.

The managed llama.cpp runtime (`usr/lib/shadowcode` in both packages) is
attributed to `llama.cpp@<commit>` and, for its Vulkan module,
`SPIRV-Headers@<commit>`, the commits pinned in `tools/llama.cpp.pin`. Their
texts under `llama.cpp-<short>/` and `SPIRV-Headers-<short>/` are the files
`scripts/build-llama.cpp.sh` copies into the runtime's `NOTICES/`
(`runtime` in `sources.json`); for the header-only libraries the license
section of the header is copied verbatim (`extract`). Packaging refuses a
runtime whose commit or notices differ from these pins.
