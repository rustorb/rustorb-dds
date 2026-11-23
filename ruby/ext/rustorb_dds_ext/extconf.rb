# Builds rustorb_dds_ext against librustorb_dds_ffi (crates/rustorb-dds-ffi),
# linked statically. RUSTORB_DDS_FFI_LIBRARY names the library,
# RUSTORB_DDS_FFI_INCLUDE the directory of rustorb/dds.h. By default: the
# ones the released gem carries for this platform
# (vendor/<platform>/librustorb_dds_ffi.a and vendor/include), else those of
# the checkout this gem is in (target/release/librustorb_dds_ffi.a after
# cargo build --release -p rustorb-dds-ffi).
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.
require "mkmf"

gem_root = File.expand_path("../..", __dir__)
root = File.expand_path("..", gem_root)
cpu = RbConfig::CONFIG["host_cpu"].sub(/\Aarm64\z/, "aarch64")
os = RbConfig::CONFIG["host_os"] =~ /darwin/ ? "macos" : "linux"
vendor = File.join(gem_root, "vendor")
vendored = File.join(vendor, "#{os}-#{cpu}", "librustorb_dds_ffi.a")
lib = ENV["RUSTORB_DDS_FFI_LIBRARY"] ||
      (File.exist?(vendored) ? vendored : File.join(root, "target", "release", "librustorb_dds_ffi.a"))
inc = ENV["RUSTORB_DDS_FFI_INCLUDE"] ||
      (File.exist?(File.join(vendor, "include", "rustorb", "dds.h")) ? File.join(vendor, "include") :
       File.join(root, "crates", "rustorb-dds-ffi", "include"))
unless File.exist?(lib)
  abort "librustorb_dds_ffi not found for #{os}-#{cpu}: #{lib} " \
        "(set RUSTORB_DDS_FFI_LIBRARY, or cargo build --release -p rustorb-dds-ffi)"
end

$INCFLAGS << " -I#{inc}"
$CFLAGS << " -std=c99 -Wall -Wextra -Wno-unused-parameter -D_POSIX_C_SOURCE=200809L"
$LOCAL_LIBS << " #{lib}"
if RUBY_PLATFORM =~ /darwin/
  $LDFLAGS << " -framework CoreFoundation -framework Security -framework SystemConfiguration"
else
  $LIBS << " -lpthread -ldl -lm"
end
create_makefile("rustorb/dds/rustorb_dds_ext")
