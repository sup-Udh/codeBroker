#!/usr/bin/env bash
set -e

REPO="sup-Udh/codeBroker"
BIN_DIR="$HOME/.codebroker/bin"
EXE_NAME="codebroker"
MCP_EXE_NAME="codebroker-mcp"

echo "Installing CodeBroker..."

# Detect OS
OS="$(uname -s)"
case "${OS}" in
    Linux*)     OS_NAME="linux";;
    Darwin*)    OS_NAME="macos";;
    *)          echo "Unsupported OS: ${OS}"; exit 1;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
    x86_64)     ARCH_NAME="x86_64";;
    amd64)      ARCH_NAME="x86_64";;
    arm64)      ARCH_NAME="aarch64";;
    aarch64)    ARCH_NAME="aarch64";;
    *)          echo "Unsupported architecture: ${ARCH}"; exit 1;;
esac

R2_URL="https://pub-aa0624d820a7465aa2d7388f8ad39d1b.r2.dev"

# Formulate asset name based on github actions workflow
ASSET_NAME="codebroker-${OS_NAME}-${ARCH_NAME}.tar.gz"
DOWNLOAD_URL="${R2_URL}/${ASSET_NAME}"

echo "Downloading latest release: $DOWNLOAD_URL"

mkdir -p "$BIN_DIR"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

curl -sL "$DOWNLOAD_URL" -o "$TMP_DIR/$ASSET_NAME"
tar -xzf "$TMP_DIR/$ASSET_NAME" -C "$TMP_DIR"
mv "$TMP_DIR/$EXE_NAME" "$BIN_DIR/$EXE_NAME"
mv "$TMP_DIR/$MCP_EXE_NAME" "$BIN_DIR/$MCP_EXE_NAME"
chmod +x "$BIN_DIR/$EXE_NAME" "$BIN_DIR/$MCP_EXE_NAME"

echo "Successfully installed $EXE_NAME and $MCP_EXE_NAME to $BIN_DIR"

# Check if it's in PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    echo "=========================================================="
    echo "Please add $BIN_DIR to your PATH."
    echo "For bash, add this to your ~/.bashrc or ~/.bash_profile:"
    echo "  export PATH=\"\$PATH:$BIN_DIR\""
    echo "For zsh, add this to your ~/.zshrc:"
    echo "  export PATH=\"\$PATH:$BIN_DIR\""
    echo "=========================================================="
fi

echo ""
echo "Installation complete! Run 'codebroker --help' to get started."
