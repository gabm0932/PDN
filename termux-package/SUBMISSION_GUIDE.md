# PDN Termux Package - Submission Guide

## Package Information
- **Name**: pdn
- **Version**: 0.1.7
- **Description**: PRoot Distro Next - Next-generation container platform for Termux/Android
- **License**: GPL-2.0
- **Homepage**: https://github.com/michaelamoetoevsky/PDN
- **Source**: https://github.com/michaelamoetoevsky/PDN/archive/refs/tags/v0.1.7.tar.gz

## Build Dependencies
- rust
- clang
- lld
- cmake
- ninja
- protobuf
- pkg-config
- libssl-dev
- libclang-dev

## Runtime Dependencies
- proot
- proot-distro

## Package Contents

### Binaries
- `podnext` - Main CLI binary (`${PREFIX}/bin/podnext`)
- `podnext-daemon` - Service supervisor daemon (`${PREFIX}/bin/podnext-daemon`)

### Services (Runit)
- `${PREFIX}/var/service/podnextd/run` - Runit service script
- `${PREFIX}/var/service/podnextd/log/run` - Service logging script

### Shell Completions
- bash: `share/bash-completion/completions/podnext`
- zsh: `share/zsh/site-functions/_podnext`
- fish: `share/fish/vendor_completions.d/podnext.fish`

### Configuration
- `/etc/pn-next/supervisor.toml` - Supervisor configuration
- `/etc/termux/boot/pdn-services` - Termux:Boot auto-start script

### Runtime Directories
- `/etc/pn-next/services` - Service definitions
- `/var/run/pn-next` - Runtime files
- `/var/log/pn-next` - Log files

## Submission Steps for termux-packages

1. Fork the termux/termux-packages repository:
   ```bash
   git clone https://github.com/termux/termux-packages.git
   cd termux-packages
   ```

2. Create the package directory and copy recipe + service files:
   ```bash
   mkdir -p packages/pdn
   cp /path/to/PDN/termux-package/recipe/build.sh packages/pdn/
   cp -r /path/to/PDN/termux-package/service packages/pdn/
   ```

3. Add the package to the build system:
   ```bash
   # Add pdn to packages.txt or appropriate package list
   echo "pdn" >> packages.txt
   ```

4. Test the build:
   ```bash
   ./build-package.sh pdn
   ```

5. Submit PR to termux/termux-packages with:
   - Title: "Add pdn package - PRoot Distro Next container platform"
   - Description: "Adds PDN (PRoot Distro Next) - a next-generation container platform for Termux/Android"

## Verification Checklist

- [ ] Package builds successfully in termux-packages CI
- [ ] Binaries run correctly on Termux/Android (`podnext`, `podnext-daemon`)
- [ ] All subcommands work (install, list, service, daemon, etc.)
- [ ] Runit service starts and logs correctly (`sv-enable podnextd`, `sv up podnextd`)
- [ ] Shell completions work
- [ ] Termux:Boot integration works
- [ ] Service supervisor daemon works

## Binary Verification

The release binaries are available at:
https://github.com/michaelamoetoevsky/PDN/releases/tag/v0.1.7

- `podnext-aarch64-linux-android` - Main CLI
- `podnext-daemon-aarch64-linux-android` - Supervisor Daemon

## Build Verification

All tests pass:
```
cargo test --workspace
```

Workspace compiles cleanly:
```
cargo check --workspace
```
