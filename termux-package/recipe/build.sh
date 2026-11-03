#!/bin/bash
# Termux package build script for PDN (PRoot Distro Next)
# This script follows termux-packages conventions

TERMUX_PKG_HOMEPAGE=https://github.com/michaelamoetoevsky/PDN
TERMUX_PKG_DESCRIPTION="PRoot Distro Next - Next-generation container platform for Termux/Android"
TERMUX_PKG_LICENSE="GPL-2.0"
TERMUX_PKG_MAINTAINER="@michaelamoetoevsky"
TERMUX_PKG_VERSION="0.1.7"
TERMUX_PKG_SRCURL=https://github.com/michaelamoetoevsky/PDN/archive/refs/tags/v${TERMUX_PKG_VERSION}.tar.gz
TERMUX_PKG_SHA256=SKIP
TERMUX_PKG_AUTO_UPDATE=true
TERMUX_PKG_DEPENDS="proot, proot-distro"
TERMUX_PKG_BUILD_DEPENDS="rust, clang, lld, cmake, ninja, protobuf, pkg-config, libssl-dev, libclang-dev"
TERMUX_PKG_BUILD_IN_SRC=true

termux_step_pre_configure() {
	# Install Rust if not present
	if ! command -v cargo &> /dev/null; then
		termux_setup_rust
	fi
}

termux_step_make() {
	# Build both podnext and podnext-daemon binaries with cargo
	cargo build --jobs ${TERMUX_PKG_MAKE_PROCESSES:-$(nproc)} --target ${CARGO_TARGET_NAME} --release \
		--package podnext --bin podnext --bin podnext-daemon
}

termux_step_make_install() {
	local bin_dir="target/${CARGO_TARGET_NAME}/release"
	[ ! -d "${bin_dir}" ] && bin_dir="target/release"

	# Install podnext binary
	install -Dm700 ${bin_dir}/podnext \
		${TERMUX_PREFIX}/bin/podnext

	# Install podnext-daemon binary
	install -Dm700 ${bin_dir}/podnext-daemon \
		${TERMUX_PREFIX}/bin/podnext-daemon

	# Install runit service script
	mkdir -p ${TERMUX_PREFIX}/var/service/podnextd/log
	if [ -f "${TERMUX_PKG_SRCDIR}/termux-package/service/podnextd/run" ]; then
		install -Dm755 "${TERMUX_PKG_SRCDIR}/termux-package/service/podnextd/run" \
			${TERMUX_PREFIX}/var/service/podnextd/run
	elif [ -f "${TERMUX_PKG_BUILDER_DIR}/service/podnextd/run" ]; then
		install -Dm755 "${TERMUX_PKG_BUILDER_DIR}/service/podnextd/run" \
			${TERMUX_PREFIX}/var/service/podnextd/run
	elif [ -f "${TERMUX_PKG_BUILDER_DIR}/../service/podnextd/run" ]; then
		install -Dm755 "${TERMUX_PKG_BUILDER_DIR}/../service/podnextd/run" \
			${TERMUX_PREFIX}/var/service/podnextd/run
	else
		cat > ${TERMUX_PREFIX}/var/service/podnextd/run << 'EOF'
#!/bin/sh
exec 2>&1
export LD_LIBRARY_PATH=""
exec podnext-daemon
EOF
		chmod 755 ${TERMUX_PREFIX}/var/service/podnextd/run
	fi

	if [ -f "${TERMUX_PKG_SRCDIR}/termux-package/service/podnextd/log/run" ]; then
		install -Dm755 "${TERMUX_PKG_SRCDIR}/termux-package/service/podnextd/log/run" \
			${TERMUX_PREFIX}/var/service/podnextd/log/run
	elif [ -f "${TERMUX_PKG_BUILDER_DIR}/service/podnextd/log/run" ]; then
		install -Dm755 "${TERMUX_PKG_BUILDER_DIR}/service/podnextd/log/run" \
			${TERMUX_PREFIX}/var/service/podnextd/log/run
	elif [ -f "${TERMUX_PKG_BUILDER_DIR}/../service/podnextd/log/run" ]; then
		install -Dm755 "${TERMUX_PKG_BUILDER_DIR}/../service/podnextd/log/run" \
			${TERMUX_PREFIX}/var/service/podnextd/log/run
	else
		cat > ${TERMUX_PREFIX}/var/service/podnextd/log/run << 'EOF'
#!/bin/sh
exec svlogd -tt /data/data/com.termux/files/usr/var/log/sv/podnextd
EOF
		chmod 755 ${TERMUX_PREFIX}/var/service/podnextd/log/run
	fi

	# Create directories for service supervisor
	mkdir -p ${TERMUX_PREFIX}/etc/pn-next/services
	mkdir -p ${TERMUX_PREFIX}/var/run/pn-next
	mkdir -p ${TERMUX_PREFIX}/var/log/pn-next

	# Install default service configuration
	cat > ${TERMUX_PREFIX}/etc/pn-next/supervisor.toml << 'EOF'
# PDN Service Supervisor Configuration
services_dir = "/data/data/com.termux/files/home/.config/pn-next/services"
runtime_dir = "/data/data/com.termux/files/home/.local/run/pn-next"
log_dir = "/data/data/com.termux/files/home/.local/log/pn-next"
supervision_interval_ms = 1000
shutdown_timeout_secs = 30
enable_cgroups = true
max_concurrent_ops = 10
EOF

	# Install shell completions
	mkdir -p ${TERMUX_PREFIX}/share/bash-completion/completions
	mkdir -p ${TERMUX_PREFIX}/share/zsh/site-functions
	mkdir -p ${TERMUX_PREFIX}/share/fish/vendor_completions.d

	# Generate completions
	${TERMUX_PREFIX}/bin/podnext completion bash > ${TERMUX_PREFIX}/share/bash-completion/completions/podnext
	${TERMUX_PREFIX}/bin/podnext completion zsh > ${TERMUX_PREFIX}/share/zsh/site-functions/_podnext
	${TERMUX_PREFIX}/bin/podnext completion fish > ${TERMUX_PREFIX}/share/fish/vendor_completions.d/podnext.fish
}

termux_step_post_make_install() {
	# Create Termux:Boot script for auto-start
	mkdir -p ${TERMUX_PREFIX}/etc/termux/boot
	cat > ${TERMUX_PREFIX}/etc/termux/boot/pdn-services << 'EOF'
#!/bin/sh
# Auto-start PDN services on Termux boot
export PN_NEXT_SERVICES_DIR=/data/data/com.termux/files/home/.config/pn-next/services
export PN_NEXT_RUNTIME_DIR=/data/data/com.termux/files/home/.local/run/pn-next
export PN_NEXT_LOG_DIR=/data/data/com.termux/files/home/.local/log/pn-next

# Start the supervisor daemon
podnext-daemon start &
EOF
	chmod +x ${TERMUX_PREFIX}/etc/termux/boot/pdn-services
}