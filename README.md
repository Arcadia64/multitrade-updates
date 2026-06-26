# MultiTrade

Multi-broker trading desktop app built with Tauri 2 (Rust) + Vue 3 (TypeScript).

Supports: Fennel, Robinhood, Public, Tastytrade, Webull.

## Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://rustup.rs/) (stable)
- [Tauri CLI prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform

## Setup

```bash
npm install
```

## Development

```bash
npm run tauri dev
```

## Build

```bash
npm run tauri build
```

The installer will be in `src-tauri/target/release/bundle/`.

## Auto Updates

The app checks for updates on startup (silently, after 2 seconds) and via the "Check Updates" button in the header. Updates are served from a separate public repo:

`https://github.com/Arcadia64/multitrade-updates/releases/latest/download/latest.json`

### Signing key

- Private key: `src-tauri/.tauri/multitrade.key` (gitignored, never commit)
- Public key: embedded in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`
- Key password: `multitrade`
- To regenerate: `npx tauri signer generate -w src-tauri/.tauri/multitrade.key -f --ci -p "YOUR_PASSWORD"`
  - Then update the `pubkey` in `tauri.conf.json` with the contents of `multitrade.key.pub`

### Release flow

1. Bump version in both `package.json` and `src-tauri/tauri.conf.json`.

2. Build with signing using the batch script:
   ```
   build-signed.bat
   ```
   This loads the signing key and password, then runs `npx tauri build`. The build produces installers + `.sig` signature files in `src-tauri/target/release/bundle/`.

   **Important:** Do NOT try to set `TAURI_SIGNING_PRIVATE_KEY` via bash env vars on Windows — the key won't propagate correctly to the Tauri CLI. Always use the batch script.

3. Create `latest.json` in the bundle directory with this structure:
   ```json
   {
     "version": "X.Y.Z",
     "notes": "Release notes here",
     "pub_date": "2026-01-01T00:00:00Z",
     "platforms": {
       "windows-x86_64": {
         "signature": "<contents of MultiTrade_X.Y.Z_x64-setup.exe.sig>",
         "url": "https://github.com/Arcadia64/multitrade-updates/releases/latest/download/MultiTrade_X.Y.Z_x64-setup.exe"
       }
     }
   }
   ```

4. Create a GitHub release on `Arcadia64/multitrade-updates` and upload 3 files:
   - `latest.json`
   - `MultiTrade_X.Y.Z_x64-setup.exe` (from `bundle/nsis/`)
   - `MultiTrade_X.Y.Z_x64-setup.exe.sig` (from `bundle/nsis/`)

   Using gh CLI:
   ```
   gh release create vX.Y.Z --repo Arcadia64/multitrade-updates --title "vX.Y.Z" --notes "Release notes" latest.json MultiTrade_X.Y.Z_x64-setup.exe MultiTrade_X.Y.Z_x64-setup.exe.sig
   ```

   **Note:** `gh` CLI is installed portably at `C:\Users\seanm2015\gh-cli\bin\gh.exe`.
