#!/bin/sh
set -e

REPO="RageLtd/cartographer"
BINARY_NAME="cartographer"
INSTALL_DIR="${HOME}/.cartographer/bin"
VERSION_FILE="${HOME}/.cartographer/.version"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Darwin)
    case "${ARCH}" in
      arm64) PLATFORM="darwin-arm64" ;;
      x86_64) PLATFORM="darwin-x64" ;;
      *) echo "Unsupported architecture: ${ARCH}"; exit 1 ;;
    esac
    ;;
  Linux)
    case "${ARCH}" in
      x86_64) PLATFORM="linux-x64" ;;
      *) echo "Unsupported architecture: ${ARCH}"; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: ${OS}"
    exit 1
    ;;
esac

ASSET_NAME="${BINARY_NAME}-${PLATFORM}"

# Get latest release tag
LATEST_TAG=$(curl -sL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')

if [ -z "${LATEST_TAG}" ]; then
  # If we can't reach GitHub but have a binary, use what we have
  if [ -x "${INSTALL_DIR}/${BINARY_NAME}" ]; then
    exit 0
  fi
  echo "Cannot determine latest version and no binary installed"
  exit 1
fi

# Check if we already have this version
if [ -f "${VERSION_FILE}" ]; then
  CURRENT_VERSION=$(cat "${VERSION_FILE}")
  if [ "${CURRENT_VERSION}" = "${LATEST_TAG}" ] && [ -x "${INSTALL_DIR}/${BINARY_NAME}" ]; then
    exit 0
  fi
fi

echo "Installing ${BINARY_NAME} ${LATEST_TAG} (${PLATFORM})..."

# Create install directory
mkdir -p "${INSTALL_DIR}"

# Download beside the binary and move it into place only once it is whole, so
# a failed download never replaces a working binary. -f makes curl fail on an
# HTTP error; without it a 404 page would be saved as the binary.
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${ASSET_NAME}"
# The process id keeps two runs apart: the SessionStart hook and the server
# command can both be installing on a first session.
DOWNLOADED="${INSTALL_DIR}/${BINARY_NAME}.download.$$"
if ! curl -fsSL "${DOWNLOAD_URL}" -o "${DOWNLOADED}"; then
  rm -f "${DOWNLOADED}"
  echo "Failed to download ${BINARY_NAME} ${LATEST_TAG} from ${DOWNLOAD_URL}"
  exit 1
fi

# Make executable
chmod +x "${DOWNLOADED}"

# Clear macOS Gatekeeper quarantine if on Darwin
if [ "${OS}" = "Darwin" ]; then
  codesign --force --deep --sign - "${DOWNLOADED}" 2>/dev/null || true
fi

mv "${DOWNLOADED}" "${INSTALL_DIR}/${BINARY_NAME}"

# Save version
mkdir -p "$(dirname "${VERSION_FILE}")"
echo "${LATEST_TAG}" > "${VERSION_FILE}"

echo "${BINARY_NAME} ${LATEST_TAG} installed successfully"
