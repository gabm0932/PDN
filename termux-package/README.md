# PDN Termux Package

This directory contains the Termux package build recipe for PDN (PRoot Distro Next).

## Package Information

- **Name**: pdn
- **Version**: 0.1.7
- **Description**: PRoot Distro Next - Next-generation container platform for Termux/Android
- **License**: GPL-2.0
- **Homepage**: https://github.com/michaelamoetoevsky/PDN

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

## Installation

The package installs:
- `podnext` - Main CLI binary (`${PREFIX}/bin/podnext`)
- `podnext-daemon` - Service supervisor daemon (`${PREFIX}/bin/podnext-daemon`)
- Runit service script (`${PREFIX}/var/service/podnextd/run`)
- Runit log service script (`${PREFIX}/var/service/podnextd/log/run`)
- Shell completions (bash, zsh, fish)
- Termux:Boot integration script
- Default configuration files

## Building

This package is designed to be built within the termux-packages build infrastructure:

```bash
# In termux-packages repo
./build-package.sh pdn
```

## Service Management (Runit / termux-services)

A runit service is installed at `${PREFIX}/var/service/podnextd`:

- Enable service: `sv-enable podnextd`
- Disable service: `sv-disable podnextd`
- Start service: `sv up podnextd`
- Stop service: `sv down podnextd`
- Status: `sv status podnextd`
- Logs: `${PREFIX}/var/log/sv/podnextd/current`

## Configuration

Default configuration files are installed to:
- `${PREFIX}/etc/pn-next/supervisor.toml` - Supervisor configuration

Runtime directories:
- `${PREFIX}/etc/pn-next/services` - Service definitions
- `${PREFIX}/var/run/pn-next` - Runtime files
- `${PREFIX}/var/log/pn-next` - Log files

## Termux:Boot Integration

A boot script is installed at `${PREFIX}/etc/termux/boot/pdn-services` to auto-start services on boot when using the Termux:Boot app.

## Shell Completions

Completions are installed for:
- bash: `${PREFIX}/share/bash-completion/completions/podnext`
- zsh: `${PREFIX}/share/zsh/site-functions/_podnext`
- fish: `${PREFIX}/share/fish/vendor_completions.d/podnext.fish`

## Submitting to termux-packages

1. Fork the termux-packages repository
2. Create a new directory `packages/pdn` with `build.sh` and `service/`
3. Submit a PR to termux-packages
4. Wait for review and merge
5. The package will be built and published to Termux repositories
