# Releasing

`.github/workflows/release.yml` builds, checks and publishes a release
from a tag `v<version>`. Every package has the same version.

## What is released

| What | Where |
| --- | --- |
| The crates `rustorb-dds-security`, `rustorb-xtypes`, `rustorb-rtps`, `rustorb-dds`, `rustorb-dds-corba` and `rustorb-dds-ffi` (in this order: each depends only on those before it) | crates.io |
| The Python package `rustorb-dds` (`python/`, with its native module in `crates/rustorb-dds-python`): one abi3 wheel (CPython 3.9 and later) for each of Linux x86_64 and aarch64 (manylinux 2.28), macOS x86_64 and arm64, and Windows x64, and the sdist | PyPI |
| `rustorb-dds-native-<version>-<platform>` (`.tar.gz`, `.zip` on Windows): `librustorb_dds_ffi` of the C ABI, shared and static, with `rustorb/dds.h`, for Linux x86_64 and aarch64 (glibc 2.28), macOS arm64 and x86_64, and Windows x64 ([docs/c-abi.md](c-abi.md)) | GitHub Release |
| The wheels, the sdist, the `.crate` files, `SHA256SUMS`, and the notes from `CHANGELOG.md` | GitHub Release `v<version>` |

`rustorb-dds-python` is never published to crates.io (`publish = false`).
The C++ and Java bindings are not packaged yet: they build from the
repository (`cpp/`, `java/`).

## The version

The version is written in the Cargo workspace (and the `version` of the
workspace's path dependencies), `python/pyproject.toml`,
`python/rustorb_dds/__init__.py`, `java/pom.xml` and
`cpp/CMakeLists.txt`. `tools/set-version.py` changes them all:

```sh
tools/set-version.py 0.3.0      # rewrite the files
cargo update --workspace        # Cargo.lock, for the --locked builds
tools/set-version.py --check    # they agree (CI checks it too)
```

Before 1.0, a new minor version may change the API.

## Once, before the first release

In the repository settings (Settings, Environments), create:

- the environment `crates-io`, with the secret `CARGO_REGISTRY_TOKEN`: a
  crates.io [API token](https://crates.io/settings/tokens) with the
  scopes `publish-new` and `publish-update`. The first upload of a new
  crate needs a token (crates.io trusted publishing can only be set up
  for a crate that exists); after it, trusted publishing could replace
  the token.
- the environment `pypi` (it exists since the Python 0.1.0 release), with
  the PyPI trusted publisher for the project `rustorb-dds`: owner
  `rustorb`, repository `rustorb-dds`, workflow `release.yml`, environment
  `pypi`. Instead, an API token in the `PYPI_API_TOKEN` secret of the
  environment also works.

Required reviewers on these environments make each upload wait for an
approval; limiting their deployment tags to `v*` is a good idea.

## Each release

1. Set the version, if it is a new one (above), and rename
   `## [x.y.z] - unreleased` in `CHANGELOG.md` to
   `## [x.y.z] - <date>` (the workflow wants a dated entry).
2. Merge to main, then tag the merge commit `vx.y.z`. From the web: on
   [a new release](https://github.com/rustorb/rustorb-dds/releases/new),
   create the tag `vx.y.z` on main and press "Publish release" (not "Save
   draft", which makes no tag); the workflow then fills that release.
3. The workflow:
   - checks that the tag is the version of every package and that
     `CHANGELOG.md` has a dated entry for it;
   - packages the crates and builds each against the others as packaged
     (`cargo publish --workspace --dry-run`), builds the wheels (and runs
     `python/tests` with each where it can), the sdist, and the native
     archives of the C ABI (and builds and runs a C program against each
     where it can);
   - attaches them all to a draft GitHub Release `vx.y.z`, with
     `SHA256SUMS` and the version's section of `CHANGELOG.md`;
   - when all of that passed: publishes the crates to crates.io, in the
     order above (job `crates-io`), and the Python package to PyPI (job
     `pypi`);
   - when both are done, publishes the GitHub Release.

If a publishing job fails, fix the cause and run the failed jobs again
from the Actions page: crates already on crates.io and files already on
PyPI are skipped. If the fix needs a change to the workflow or the source,
merge it, delete the release and the tag `vx.y.z`, and tag again (only
while nothing is published yet; otherwise release the next patch
version).

Nothing published can be taken back: crates.io can only yank a version,
and PyPI never takes the same version again.

Pull requests that change the packaging, and Run workflow, build and
check everything without publishing.

## The Python package 0.1.0

The Python package alone was released as 0.1.0, from the tag
`python-v0.1.0`, with an earlier version of this workflow.
