# Quick Start: Cross-Platform Build

## One-Command Builds

Recommended: use helper scripts to build binaries for multiple targets.

### Linux/macOS

```bash
# Build all targets
./scripts/build-cross.sh

# Build specific targets
./scripts/build-cross.sh x86_64-unknown-linux-gnu x86_64-apple-darwin
```

### Windows

```cmd
REM Build all targets
scripts\build-cross.bat

REM Build specific targets
scripts\build-cross.bat x86_64-pc-windows-msvc
```

## CI Builds

GitHub Actions workflow builds on push/PR/tags, uploads artifacts, and can create releases.

## Verify Artifacts

```bash
./scripts/verify-build.sh
```

## Artifacts Location

- Binaries are copied to `target/releases/`
- CI produces `.tar.gz` (Linux/macOS) or `.zip` (Windows)

## Supported Targets

- Linux: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`
- macOS: `x86_64-apple-darwin`, `aarch64-apple-darwin`
- Windows: `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`

## More Docs

- CROSS-COMPILE-GUIDE.md
- README.md

