# Stella 2

[![Build Status](https://yvt.visualstudio.com/Stella2/_apis/build/status/yvt.Stella2?branchName=master)](https://yvt.visualstudio.com/Stella2/_build/latest?definitionId=1&branchName=master)

TODO

## Directory structure

    stella2
     │
     ├╴ci               CI configuration
     │
     ├╴stella2          The main program
     │
     ├╴stellca2_assets
     │
     ├╴res              Things related to metadata attached to Stella2's executable
     │  │
     │  ├╴mkmacosbundle A command-line utility for creating a macOS application
     │  │               bundle (This program is invoked from `/build-mac.sh`.)
     │  │
     │  └╴windres       Windows resources (application icon, manifest, etc.)
     │
     ├╴tcw3             TCW3, the GUI framework
     │
     ├╴harmony          A state management library
     │
     ├╴stvg             A library for encoding/decoding vector images
     │
     └╴support          An assortment of supporting libraries

## Testing

TCW3 includes a headless backend named `testing`, which is provided for testing purposes. It's disabled by default because of the additional dependencies it introduces. Use the feature flag `tcw3/testing` to enable it:

     cd tcw3
     cargo test --features testing

When testing the whole workspace, specifying features on individual crates won't work ([rust-lang/cargo#6195]), so pass `--all-features` instead:

     cargo test --workspace --all-features

[rust-lang/cargo#6195]: https://github.com/rust-lang/cargo/issues/6195

## Prerequisites

The nightly Rust compiler is required. Depending on when you are reading this, a stable compiler might work.

When building for a Linux system or using TCW3's `testing` backend, dependent crates expect **GLib**, **Cairo**, and **Pango** development files to be installed on your system.

Fedora:

```shell
sudo yum install glib2-devel cairo-devel cairo-gobject-devel pango-devel
```

Nix:

```shell
# Assumes `cargo` and the nightly toolchain are already available.
#
# `x11-rs` searches additional paths if non-standard paths are detected during
# compile time. For this reason, `xorg.*` must be available when building.
# See <https://github.com/erlepereira/x11-rs/pull/46> for more about this
# behavior.
nix-shell -p glib pkgconfig pango xorg.libXcursor xorg.libXrandr xorg.libXi \
  --command 'cargo build --release -p stella2'
```

## Third-party software

This source tree includes the following third-party projects:

 - (Git subtree) <https://github.com/yvt/alt_fp-rs> at `support/alt_fp`
 - `stvg_macro` is partly based on [Pathfinder 3](https://github.com/servo/pathfinder), licensed by the Pathfinder Project developers under the Apache License, Version 2.0 or the MIT license.

## License

TBD

