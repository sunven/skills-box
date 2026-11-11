# Signed macOS releases

Application releases are public stable versions from `sunven/skills-box`.
The updater reads `releases/latest/download/latest.json` without the user's
Skill-source GitHub credentials. Existing applications built before updater
support require one manual installation of a release that includes it.

## Signing key

The updater public key is in `src-tauri/tauri.conf.json`. Its matching private
key and randomly generated password are outside the repository, by default:

```text
~/.local/share/skills-box/signing/updater.key
~/.local/share/skills-box/signing/updater.key.pub
~/.local/share/skills-box/signing/updater.password
```

Files have Unix `0600` permissions; a new directory has `0700` permissions.
Back up the private key and password securely: changing the public key alone
does not update the trust root in applications already installed.

`pnpm updater:keygen` creates this application's key pair and configures its
public key. Reruns reuse the existing complete key pair. A different configured
key or an incomplete directory fails rather than replacing an existing key.
An optional directory argument and its resolved symlink destination must be
outside the repository, including when a new child directory is requested.

For another maintainer or CI, supply the matching private key; do not generate
a replacement key for an already published updater. The key is unrelated to
Apple Developer signing and notarization, which remain outside this release.

## Local build

```bash
pnpm install --frozen-lockfile
pnpm build:signed --bundles app
# Include a first-install DMG, or cross-build an architecture explicitly:
pnpm build:signed --target aarch64-apple-darwin --bundles app,dmg
pnpm build:signed --target x86_64-apple-darwin --bundles app,dmg
```

The helper reads the private key and password directly into the build's
environment. `SKILLS_BOX_SIGNING_DIR` can point at a different external signing
directory. It does not display credentials or pass them through shell expansion.

On 2026-11-04, the local Apple Silicon build completed, but its signing key
did not match the committed updater public key. The external `.pub` file also
differs from the configuration. These local artifacts have not passed update
signature validation. Supply the matching private key before publishing;
changing the public key requires an explicit trust-root rotation.

Verify an update archive against the committed public key:

```bash
cd src-tauri
cargo run --locked --example verify-update -- \
  "target/release/bundle/macos/Skills Box.app.tar.gz" \
  "target/release/bundle/macos/Skills Box.app.tar.gz.sig"
```

## GitHub prerequisites

The target repository must be accessible and its Releases publicly readable.
At implementation time, authenticated and anonymous queries to
`sunven/skills-box` returned 404, so remote configuration and live update
delivery could not be verified. The authenticated repository query still
returned 404 on 2026-11-04.

Once the repository exists and the current account has access, set the two
Actions secrets from the matching external files, without printing them:

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo sunven/skills-box \
  < ~/.local/share/skills-box/signing/updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo sunven/skills-box \
  < ~/.local/share/skills-box/signing/updater.password
```

The release job uses Actions' own `GITHUB_TOKEN` for upload permission. No user
PAT, Apple certificate or notarization credentials are required for this setup.

## Version and release

Run the release script from a clean checkout with Node.js and Git available
and an `origin` remote configured:

```bash
./scripts/release.sh        # Increment PATCH automatically, e.g. 0.1.0 → 0.1.1
./scripts/release.sh 0.2.0  # Use an explicit stable version
# Equivalent package commands:
pnpm release
pnpm release 0.2.0
```

Release preparation follows `/Users/sunven/github/fanyifanyi/scripts/release.sh`:

1. `scripts/bump-version.sh` updates `package.json`, `src-tauri/tauri.conf.json`
   and `src-tauri/Cargo.toml`, then runs `cargo update --workspace` for the lockfile.
2. `git-cliff --tag vX.Y.Z -o CHANGELOG.md` generates release notes using the
   reference project's `cliff.toml`. Install it with `brew install git-cliff`.
   If it is unavailable, the script preserves any existing manual changelog.
3. Commit the version files and `CHANGELOG.md` when present.
4. Create the annotated `vX.Y.Z` tag.
5. Confirm before pushing the current branch and tag to `origin`.

The script validates matching versions across all four version files. It
retains clean-checkout, duplicate-tag and downgrade checks and pushes the
branch and tag atomically. Declining or reaching EOF keeps the local commit
and tag and prints the command for a later push. `pnpm-lock.yaml` does not
store the application version and does not need updating for a version bump.

Pre-release versions, lower versions, inconsistent versions, an existing local
tag, detached HEAD and uncommitted changes (including untracked files) stop the
script before it changes version files. An explicit version equal to the current
version is allowed for a first release; it commits a generated changelog if
needed and avoids an empty commit. Ignored local signing keys are not included
in release commits. GitHub release notes are also generated by the Action.

The workflow follows `/Users/sunven/github/fanyifanyi/.github/workflows/release.yml`:
one macOS Apple Silicon job first runs `pnpm test`, `pnpm test:release`, and
`cargo test --locked` in `src-tauri`. Any test failure stops the job before
building or publishing. It then invokes `tauri-apps/tauri-action@v1` with
`--target aarch64-apple-darwin --bundles dmg,app`. The Action builds and signs
the app, uploads its assets, generates `latest.json` and publishes directly
with `releaseDraft: false`, `prerelease: false`, `generateReleaseNotes: true`,
`uploadUpdaterJson: true` and `retryAttempts: 2`.

The next step downloads the published `latest.json` and checks its version
and `darwin-aarch64` URL/signature. This check occurs after publication;
there is no separate draft or artifact aggregation job. Automatic releases
currently contain only Apple Silicon updates. Local Intel cross-builds remain
available through the local build commands above.

Repository names and the changelog's `main` branch URL are adapted to Skills
Box. CI uses Node 22 and the pinned `pnpm@11.10.0` from `package.json`, since
this pnpm version requires Node 22.13 or later. The updater's existing public
key and signed-version requirement continue to protect installation.

Release validation tests run with:

```bash
pnpm test:release
```

To verify live delivery, install an older signed application containing the
updater, publish a higher stable version, then use its update settings to check,
download, install and restart. A real-machine or CI test is required before
claiming a complete online upgrade test.
