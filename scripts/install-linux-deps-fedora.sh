#!/usr/bin/env bash
set -euo pipefail

# Install the native Tauri build prerequisites and RPM packaging tools with
# dnf. Works on Fedora out of the box; on Enterprise Linux 10 systems
# (RHEL, Rocky, Alma, CentOS Stream, Oracle) it enables EPEL 10 and CRB
# first, since webkit2gtk4.1-devel is not in the default Enterprise Linux
# repositories (and not in EPEL 9, so EL 9 and older are unsupported).
#
# Usage: npm run setup:linux:fedora

if ! command -v dnf >/dev/null 2>&1; then
  echo "This helper currently supports Fedora and Enterprise Linux 10 systems with dnf." >&2
  exit 1
fi

if [ "$(id -u)" -eq 0 ]; then
  SUDO=()
elif command -v sudo >/dev/null 2>&1; then
  SUDO=(sudo)
else
  echo "sudo is required when not running as root." >&2
  exit 1
fi

# Enterprise Linux keeps webkit2gtk4.1-devel in EPEL 10 (not in the default
# repos, not in EPEL 9), with some -devel dependencies in CRB. Fedora needs
# neither, so this block is a no-op there. Each distribution needs its own
# repository names, so they are handled per ID below.
distro=""
if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  # Intentional word splitting over ID_LIKE (e.g. "rhel centos fedora").
  # shellcheck disable=SC2086
  for token in ${ID:-} ${ID_LIKE:-}; do
    case "$token" in
      rhel|centos|rocky|almalinux|ol|circle) distro="${ID:-}" ;;
    esac
  done
fi
if [ -n "$distro" ]; then
  # Only EL 10 is supported: webkit2gtk4.1-devel exists in EPEL 10 only.
  major="${VERSION_ID%%.*}"
  if [ "${PLATFORM_ID:-}" != "platform:el10" ] && [ "${major:-}" != "10" ]; then
    echo "Only Fedora and Enterprise Linux 10 are supported (found ${PRETTY_NAME:-unknown})." >&2
    exit 1
  fi
  # The conditional expansions also support empty arrays with nounset on Bash 3.
  case "$distro" in
    rhel)
      # epel-release is not in BaseOS/AppStream; install the major-pinned
      # package over HTTPS. CRB is a subscription-manager repo on RHEL
      # (RHUI cloud images use the -rhui-rpms variant).
      arch="$(uname -m)"
      if command -v subscription-manager >/dev/null 2>&1; then
        "${SUDO[@]+"${SUDO[@]}"}" subscription-manager repos --enable "codeready-builder-for-rhel-10-${arch}-rpms" || \
        "${SUDO[@]+"${SUDO[@]}"}" subscription-manager repos --enable "codeready-builder-for-rhel-10-${arch}-rhui-rpms"
      fi
      "${SUDO[@]+"${SUDO[@]}"}" dnf install -y https://dl.fedoraproject.org/pub/epel/epel-release-latest-10.noarch.rpm
      ;;
    ol)
      "${SUDO[@]+"${SUDO[@]}"}" dnf install -y oracle-epel-release-el10 dnf-plugins-core
      "${SUDO[@]+"${SUDO[@]}"}" dnf config-manager --set-enabled ol10_codeready_builder ol10_developer_EPEL
      ;;
    rocky|almalinux|centos|circle)
      # epel-release ships in extras and the CRB repo id really is "crb"
      # here, but minimal images lack dnf-plugins-core (which provides
      # dnf config-manager) and the crb helper that epel-release installs.
      "${SUDO[@]+"${SUDO[@]}"}" dnf install -y epel-release dnf-plugins-core
      if command -v crb >/dev/null 2>&1; then
        "${SUDO[@]+"${SUDO[@]}"}" crb enable
      else
        "${SUDO[@]+"${SUDO[@]}"}" dnf config-manager --set-enabled crb
      fi
      ;;
  esac
fi

# The conditional expansions also support empty arrays with nounset on Bash 3.
# libxdo is deliberately absent: it is only needed for Tauri's opt-in
# linux-libxdo feature, which this project does not enable (and it is not
# packaged in EPEL 10).
"${SUDO[@]+"${SUDO[@]}"}" dnf install -y \
  gcc \
  gcc-c++ \
  make \
  curl \
  wget \
  file \
  gtk3-devel \
  webkit2gtk4.1-devel \
  javascriptcoregtk4.1-devel \
  libsoup3-devel \
  openssl-devel \
  librsvg2-devel \
  libappindicator-gtk3-devel \
  patchelf \
  rpm-build
