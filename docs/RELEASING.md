# Releasing Pinscape-Cross

Releases are built by GitHub Actions ([`.github/workflows/release.yml`](../.github/workflows/release.yml))
when a `v*` tag is pushed. The workflow attaches everything to a **draft** release; you
check it and publish it by hand.

What a release contains:

| Platform | Installers | Updater bundle (with signing key) | CLI |
|---|---|---|---|
| Linux x86_64 | `.deb`, `.rpm`, `.AppImage` | `.AppImage` + `.sig` | `pinscape-cli-<ver>-linux-x86_64.tar.gz` |
| Windows x86_64 | `.msi`, NSIS `-setup.exe` | `.msi`/`.exe` + `.sig` | `pinscape-cli-<ver>-windows-x86_64.zip` |
| macOS Apple silicon | `_aarch64.dmg` | `.app.tar.gz` + `.sig` | `pinscape-cli-<ver>-macos-aarch64.tar.gz` |
| macOS Intel | `_x64.dmg` | `.app.tar.gz` + `.sig` | `pinscape-cli-<ver>-macos-x86_64.tar.gz` |

plus `latest.json`, the update manifest the app reads (only when updater signing is set up).

## One-time setup

### 1. Updater signing key

The in-app updater only installs files signed with your key. Generate a key pair once,
**outside the repository**:

```bash
npm run tauri signer generate -- -w ~/.tauri/pinscape-cross.key
```

It asks for a password (you may leave it empty) and writes
`~/.tauri/pinscape-cross.key` (private) and `~/.tauri/pinscape-cross.key.pub` (public).

- Keep the private key and its password safe and backed up. **Never commit them.** If the
  key is lost, installed copies can no longer be updated in-app; users would have to
  install the next version by hand (which then carries a new public key).
- The public key is not secret.

### 2. GitHub secrets

In the repository: **Settings > Secrets and variables > Actions > New repository secret**:

| Secret | Value |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | contents of `~/.tauri/pinscape-cross.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password (leave the secret out if the password is empty) |

### 3. Public key in `tauri.conf.json`

Paste the contents of `~/.tauri/pinscape-cross.key.pub` (one line of base64) into
[`src-tauri/tauri.conf.json`](../src-tauri/tauri.conf.json):

```json
"plugins": {
  "updater": {
    "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6...",
    "endpoints": ["https://github.com/superhac/pinscape-cross/releases/latest/download/latest.json"],
    ...
```

Commit this. Only builds made after this change check for updates.

#### How the updater behaves without a key

- The committed config ships with `"pubkey": ""` and `"createUpdaterArtifacts": false`.
  The `plugins.updater` section must stay (the updater plugin refuses to start without
  it), but with an empty key the app never contacts the update server: `updater_status`
  ([`src-tauri/src/updater.rs`](../src-tauri/src/updater.rs)) reports updates as disabled,
  so the UI makes no check and nothing can be downloaded or installed.
- `createUpdaterArtifacts` stays `false` in the repository because `tauri build` with it on
  fails unless `TAURI_SIGNING_PRIVATE_KEY` is set; that would break `npm run tauri build`
  for contributors and in CI. The release workflow switches it on for its own build when
  both the secret and the pubkey are present.
- A tagged release **fails** if only one of the two (secret, pubkey) is present, because
  publishing it would leave existing installs without a `latest.json` to update from.
  Without either, releases are built normally, without updater files.
- Debug builds (`npm run tauri dev`) never check; set `PINSCAPE_UPDATER_DEV=1` to test the
  notice. Users and packagers can turn checks off with `PINSCAPE_NO_UPDATE_CHECK=1`.

### 4. Optional: code signing

Without these secrets the installers are unsigned (macOS: ad-hoc signed). Windows
SmartScreen and macOS Gatekeeper then warn on first launch; the
[user guide](USER_GUIDE.md#installing) explains how to get past that.

**macOS** (needs a paid Apple Developer account and a *Developer ID Application*
certificate):

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | the certificate exported from Keychain Access as `.p12`, base64-encoded: `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` export password |
| `APPLE_SIGNING_IDENTITY` | optional, e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | notarization with your Apple ID and an [app-specific password](https://support.apple.com/en-us/102654) |
| or `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_PRIVATE_KEY` | notarization with an App Store Connect API key (issuer ID, key ID, contents of the `.p8` file) |

Notarization credentials are only used together with a certificate.

**Windows** (an Authenticode code-signing certificate as `.pfx`):

| Secret | Value |
|---|---|
| `WINDOWS_CERTIFICATE` | the `.pfx`, base64-encoded: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))` |
| `WINDOWS_CERTIFICATE_PASSWORD` | its password |

The workflow imports the certificate and sets `bundle.windows.certificateThumbprint`
(SHA-256, DigiCert timestamp server) for that build only. Certificates kept in a cloud HSM
(Azure Trusted Signing, etc.) need a `bundle.windows.signCommand` instead; see the
[Tauri Windows signing guide](https://v2.tauri.app/distribute/sign/windows/).

## Making a release

1. **Update the changelog.** Move the entries under `## [Unreleased]` in
   [`CHANGELOG.md`](../CHANGELOG.md) into a new `## [x.y.z] - YYYY-MM-DD` section and add
   the compare link at the bottom. This section becomes the release notes and the text in
   the app's update notice.

2. **Bump the version** everywhere it lives (workspace `Cargo.toml`, `Cargo.lock`,
   `src-tauri/tauri.conf.json`, `package.json`, `package-lock.json`):

   ```bash
   scripts/bump-version.sh 0.2.0
   scripts/bump-version.sh --check      # prints 0.2.0 if all copies agree
   ```

   Use plain `x.y.z` versions. The Windows `.msi` format only accepts numeric pre-release
   suffixes (`0.2.0-1`), so `0.2.0-beta.1` fails the Windows build. Versions with a `-`
   are published as GitHub pre-releases, which `releases/latest` (and so the updater) ignores.

3. **Check, commit, tag, push:**

   ```bash
   cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && npm run build
   git commit -am "Release 0.2.0"
   git tag v0.2.0
   git push origin master v0.2.0
   ```

   The workflow refuses a tag that doesn't match the version in the files.

4. **Wait for the Release workflow** (Actions tab, about 20-30 minutes). It creates the draft
   release `Pinscape-Cross v0.2.0` and the four build jobs upload their files. A failed job
   can be re-run; uploads replace the earlier files of the same name.

5. **Check the draft** (Releases page): all installers, the four CLI archives, and, with
   updater signing, `latest.json` listing `linux-x86_64`, `windows-x86_64`,
   `darwin-aarch64` and `darwin-x86_64`. Download and try an installer or two.

6. **Publish** the draft. From that moment
   `https://github.com/superhac/pinscape-cross/releases/latest/download/latest.json`
   points to it, and installed copies show the update notice on their next start.

To rebuild a tag's draft without pushing again: **Actions > Release > Run workflow**, enter
the tag. Leave the tag empty for a dry run that only produces workflow artifacts. A
published release is never changed by the workflow; delete it (or make a new version) first.

## Testing the updater

1. Set up signing as above and release a version (say 0.2.0).
2. Install 0.2.0 from its installer (AppImage, `.msi`/`.exe` or `.dmg`; `.deb`/`.rpm`
   installs get a "Download" button instead of in-place install).
3. Release 0.2.1 and publish it.
4. Start 0.2.0: after a few seconds the sidebar shows "Update available: 0.2.1" with the
   release notes. **Install and restart** downloads, verifies the signature, installs and
   restarts into 0.2.1.
