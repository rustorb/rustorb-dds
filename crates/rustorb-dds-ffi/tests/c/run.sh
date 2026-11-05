#!/bin/sh
# Builds the C ABI and runs smoke.c against it, as C (C11) and as C++
# (the header in a C++ translation unit).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)
cargo build --manifest-path "$root/Cargo.toml" -p rustorb-dds-ffi --release --locked
lib="$root/target/release/librustorb_dds_ffi.a"
inc="$root/crates/rustorb-dds-ffi/include"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
libs="-lpthread -ldl -lm"
${CC:-cc} -std=c11 -Wall -Wextra -Werror -I"$inc" "$here/smoke.c" "$lib" $libs -o "$out/smoke"
"$out/smoke"
${CXX:-c++} -std=c++17 -Wall -Wextra -Werror -x c++ -I"$inc" "$here/smoke.c" -x none "$lib" $libs -o "$out/smoke_cxx"
"$out/smoke_cxx"
