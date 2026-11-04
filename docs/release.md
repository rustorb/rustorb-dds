# Releasing

## The Python package

The Python package `rustorb-dds` (`python/`, with its native module in
`crates/rustorb-dds-python`) is built and published by
`.github/workflows/release.yml`:

- one abi3 wheel (CPython 3.9 and later) for each of Linux x86_64 and
  aarch64 (manylinux 2.28), macOS x86_64 and arm64, and Windows x64, each
  tested with `python/tests` where it can run, and the sdist;
- on a tag `python-v<version>`: a draft GitHub Release with those files,
  the upload to PyPI, then the release made public.

A pull request that changes the packaging, or Run workflow, only builds.

### Once, before the first release

1. On PyPI, add a trusted publisher for the project `rustorb-dds` (a
   "pending publisher" while the project does not exist yet): owner
   `rustorb`, repository `rustorb-dds`, workflow `release.yml`, environment
   `pypi`.
2. In the repository settings, create the environment `pypi` (with the
   reviewers who must approve an upload, if any).

Instead of the trusted publisher, an API token in the `PYPI_API_TOKEN`
secret of the `pypi` environment also works.

### Each release

1. Set the version in `python/pyproject.toml` and in `__version__` of
   `python/rustorb_dds/__init__.py`.
2. Move the entries of `## [Unreleased]` in `CHANGELOG.md` to
   `## [python-<version>] - <date>`.
3. Merge, then tag the merge commit `python-v<version>` and push the tag.

If a job fails, fix it and run the workflow again on the tag: files
already on PyPI are skipped.

## The Rust crates

Not published yet (`rustorb-dds`, `rustorb-rtps`, `rustorb-xtypes`; the
`rustorb-dds-python` crate is never published).
