# Releasing

This is the maintainer runbook for cutting a elicitation release. A tag
`vX.Y.Z` on `main` runs
[`.github/workflows/release.yml`](../.github/workflows/release.yml), which
builds and publishes the GitHub release, the container image and the MCP
registry entry. crates.io is published by hand afterwards.

Throughout, `X.Y.Z` is the new version and `vX.Y.Z` its tag.

## 1. Bump the version

On a branch off `dev`, set the same version in the files that carry it:

- `Cargo.toml`: the `[workspace.package] version` inherited by both crates.
- `crates/elicitation/Cargo.toml`: the `version` in the `eliciter-core`
  dependency (`eliciter-core = { path = "../elicitation-core", version =
  "X.Y.Z" }`).
- `Cargo.lock`: run `cargo check` (or `cargo update -p eliciter -p
  eliciter-core`) so the lock file records the new versions.
- `server.json`: the top-level `version` and the tag of the OCI identifier
  (`ghcr.io/praxec/elicitation:X.Y.Z`). The OCI package has no `version`
  field: the MCP registry rejects one.
- `npm/package.json`: the `version` of the `@matthew-cochran/elicitation`
  launcher. Its `files` list must keep `skills/` so the bundled agent skills
  ship in the tarball.

Then check them:

```sh
sh scripts/check-version-sync.sh
```

It fails when any `server.json` version or OCI identifier tag, the
`eliciter-core` dependency version, or the npm launcher version differs from
the workspace `Cargo.toml`. The release workflow repeats the check against the
tag (`scripts/check-version-sync.sh --tag vX.Y.Z`) and refuses to build on a
mismatch. Nothing else needs a version bump: the README writes `<version>` for
a release tag instead of pinning one.

## 2. Update the CHANGELOG

In [CHANGELOG.md](../CHANGELOG.md), rename `## [Unreleased]` to
`## [X.Y.Z] - YYYY-MM-DD` and add a fresh empty `## [Unreleased]` above it.
Follow Keep a Changelog: group entries under Added, Changed, Fixed and so on.
The date here is the planned release date; step 3 corrects it on the day.

Open the bump as a pull request into `dev` and merge it once CI is green.

## 3. Release PR from `dev` to `main`

On release day, first set the `## [X.Y.Z] - YYYY-MM-DD` date in
`CHANGELOG.md` to today's date (a one-line pull request into `dev`) if it
differs from the planned one.

Then open a pull request with base `main` and head `dev`. The gitflow guard
(`gitflow-guard.yml`) rejects any other head branch. Wait for every required
check, then merge it with a **merge commit**, not squash or rebase, so `main`
and `dev` keep a shared history.

## 4. Tag `main`

```sh
git fetch origin
git checkout main
git pull --ff-only origin main
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

Tag only commits on `main`: the workflow triggers on any `v*` tag. The tag must
match `vMAJOR.MINOR.PATCH` (an optional pre-release or build suffix is
allowed) and equal the `Cargo.toml` and `server.json` versions.

The workflow then runs these jobs:

1. **validate + draft release**: runs `scripts/check-version-sync.sh --tag`
   against the versions and creates a draft GitHub release with generated
   notes.
2. **build**: six targets (Linux, macOS and Windows on x86_64 and ARM64). Each
   archive is smoke-tested with `scripts/mcp-smoke.mjs` (MCP `initialize` and
   `tools/list`) and uploaded to the draft.
3. **aggregate metadata + publish**: writes `checksums.sha256` and
   `release-manifest.json` (it fails if the matrix is incomplete), uploads them
   with `install.sh` and `install.ps1`, syncs the agent skills into `npm/` and
   packs the launcher into `matthew-cochran-elicitation-X.Y.Z.tgz`, attaches the
   tarball and its `npm-package.sha256` digest, and only then publishes the
   release.
4. **npm publish** (`npm` job): skipped unless the repository variable
   `NPM_TRUSTED_PUBLISHING` is `true`. It downloads the attached tarball,
   verifies its digest, publishes it over npm trusted publishing (GitHub OIDC,
   no stored token), and verifies the published launcher reports `X.Y.Z`. See
   step 7 for the one-time manual first publish.
5. **container image** (`.github/workflows/image.yml`): downloads the two
   Linux release archives, checks them against `checksums.sha256` and builds
   the image from those binaries with `Dockerfile.release`, so nothing is
   compiled under emulation. It smoke-tests the amd64 image over MCP, then
   pushes `ghcr.io/praxec/elicitation` for linux/amd64 and linux/arm64
   with the tags `X.Y.Z`, `X.Y` and `latest`.
6. **MCP registry**: publishes `server.json` as
   `io.github.praxec/elicitation` with a pinned, checksum-verified
   `mcp-publisher` over GitHub OIDC. This job is best-effort
   (`continue-on-error`), so a failure does not fail the release; check its log.

## 5. Verify the release

Assets: the release page lists six archives, `checksums.sha256`,
`release-manifest.json`, `install.sh`, `install.ps1`, and — after step 3 packs
it — `matthew-cochran-elicitation-X.Y.Z.tgz` with `npm-package.sha256`.

```sh
gh release view vX.Y.Z --json assets --jq '.assets[].name'
gh release download vX.Y.Z --pattern checksums.sha256 --pattern '*linux-gnu.tar.gz'
sha256sum -c --ignore-missing checksums.sha256
```

Installer, on a clean shell (repeat with `install.ps1` on Windows):

```sh
curl -fsSL https://github.com/praxec/elicitation/releases/latest/download/install.sh | sh -s -- --version vX.Y.Z
node scripts/mcp-smoke.mjs ~/.local/bin/elicitation
```

Container image:

```sh
docker pull ghcr.io/praxec/elicitation:X.Y.Z
node scripts/mcp-smoke.mjs docker run -i --rm ghcr.io/praxec/elicitation:X.Y.Z
```

MCP registry: confirm the registry job succeeded, or look the server up in the
registry and check that its version is `X.Y.Z`.

## 6. Publish to crates.io (manual)

The workflow does not publish the crates. From the tagged commit, with a
crates.io token configured (`cargo login`), publish the library first and the
binary second — `eliciter` depends on a crates.io release of `eliciter-core`
at the same version, so publishing the binary first would fail to resolve it:

```sh
git checkout vX.Y.Z
cargo publish --dry-run --locked -p eliciter-core
cargo package --list --locked -p eliciter-core
cargo publish --locked -p eliciter-core

cargo publish --dry-run --locked -p eliciter
cargo package --list --locked -p eliciter
cargo publish --locked -p eliciter
```

The `eliciter` dependency pins `version = "X.Y.Z"` with a path override, so
`cargo publish` rewrites it to the registry when packaging; `eliciter-core`
must therefore already be live before `eliciter` is published. Until this step
runs, crates.io keeps serving the previous versions. docs.rs builds the new
versions on its own a few minutes after publishing.

## 7. Publish to npm (manual first, then trusted publishing)

The release workflow packs the launcher and attaches it to the GitHub release
(step 3), but only publishes it to npm when the repository variable
`NPM_TRUSTED_PUBLISHING` is `true`. The very first publish is manual so the
package and its trusted publisher exist on npm. From the tagged commit, with
`npm login` and `gh auth login` done:

```sh
git checkout vX.Y.Z
bash scripts/npm-release.sh X.Y.Z
```

`scripts/npm-release.sh` refuses unless you are logged in to npm and the GitHub
release `vX.Y.Z` exists and is not a draft, downloads the attached tarball,
checks it against `npm-package.sha256` and that it contains only the launcher
and bundled skills (including `package/skills/elicit-interview/SKILL.md`),
publishes it with a 2FA OTP prompt (pre-releases go to the `next` dist-tag),
and waits until the registry serves the version and `npx` reports
`elicitation X.Y.Z`.

After that first publish, register the trusted publisher on npmjs.com for
`@matthew-cochran/elicitation` (organization `praxec`, repository
`elicitation`, workflow `release.yml`, environment `npm`) and set the
repository variable:

```sh
gh variable set NPM_TRUSTED_PUBLISHING --body true
```

Subsequent releases publish automatically from the `npm` job; the manual script
remains the fallback and skips a version already on npm.

## Rollback

- **A build failed before publishing.** The release is still a draft. Fix the
  problem on `dev`, then delete the draft and the tag and re-release with the
  next patch version (or re-push the same tag if nothing was published):

  ```sh
  gh release delete vX.Y.Z --yes
  git push origin :refs/tags/vX.Y.Z
  git tag -d vX.Y.Z
  ```

  Re-running the workflow on an existing draft is also safe: assets are
  uploaded with `--clobber`, and an existing release is left as is.
- **A bad release was published.** Do not reuse its version. Mark the GitHub
  release as a pre-release or edit its notes to warn users, then ship a fixed
  `X.Y.(Z+1)`. The installers resolve `releases/latest`, so they pick up the
  fix as soon as it is published.
- **A bad npm version was published.** npm versions cannot be overwritten.
  Unpublish it within 72 hours (`npm unpublish
  @matthew-cochran/elicitation@X.Y.Z`) or, after that window, deprecate it
  (`npm deprecate`) and ship a fixed `X.Y.(Z+1)`.
- **A bad crate was published.** crates.io versions cannot be deleted or
  overwritten. Yank it so new lock files do not pick it up, then publish a fix:

  ```sh
  cargo yank --version X.Y.Z eliciter-core
  cargo yank --version X.Y.Z eliciter
  # undo: cargo yank --version X.Y.Z --undo eliciter-core
  #       cargo yank --version X.Y.Z --undo eliciter
  ```

- **The image job failed or timed out.** The GitHub release is not affected.
  Rebuild the image for the existing release with
  `gh workflow run image.yml --ref main -f tag=vX.Y.Z`, which also runs the
  MCP registry step.
- **A bad image was pushed.** Push the fixed release, which moves `latest` and
  `X.Y`. Delete the bad `X.Y.Z` tag from the GHCR package settings only if it is
  harmful to keep.
- **A bad MCP registry entry.** Publish the fixed version; the registry lists
  the newest version.
