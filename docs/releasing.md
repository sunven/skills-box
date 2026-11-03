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
An optional directory argument must be outside the repository.

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

Verify an update archive against the committed public key:

```bash
cd src-tauri
cargo run --locked --example verify-update -- \
  target/release/bundle/macos/skills-box.app.tar.gz \
  target/release/bundle/macos/skills-box.app.tar.gz.sig
```

## GitHub prerequisites

The target repository must be accessible and its Releases publicly readable.
At implementation time, authenticated and anonymous queries to
`sunven/skills-box` returned 404, so remote configuration and live update
delivery could not be verified.

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

Set the same stable version in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`, update the lock files, then push the matching
`vX.Y.Z` tag. Pre-release tags and mismatched versions fail validation.

The workflow builds Apple Silicon and Intel `.app` archives, signatures and
DMGs as private Actions artifacts. Only after both builds succeed does the
publish job prepare a draft Release. It verifies each archive's signature
against the committed public key, actual binary architecture and app version,
then creates one complete `latest.json` with unique filenames for both targets.
Signatures must carry the same application version; production configuration
requires this signed version and rejects mismatched metadata.
It uploads these to the draft, confirms the assets exist with matching sizes,
and publishes the stable Release. A failed build or validation leaves the last
public stable update intact; a rerun can resume a draft but will not modify an
already public release.

Release validation tests run with:

```bash
pnpm test:release
```

To verify live delivery, install an older signed application containing the
updater, publish a higher stable version, then use its update settings to check,
download, install and restart. Both architectures require real-machine or CI
validation before claiming a complete online upgrade test.
