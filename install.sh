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

R2_URL="https://www.codebroker.space"

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
    EXPORT_LINE="export PATH=\"\$PATH:\$HOME/.codebroker/bin\""
    echo ""
    echo "=========================================================="
    
    ADDED_TO_PROFILE=false
    
    if [ -f "$HOME/.bashrc" ]; then
        if ! grep -q "\.codebroker/bin" "$HOME/.bashrc"; then
            echo "" >> "$HOME/.bashrc"
            echo "# codebroker path" >> "$HOME/.bashrc"
            echo "$EXPORT_LINE" >> "$HOME/.bashrc"
            echo "Added codebroker to PATH in ~/.bashrc"
            ADDED_TO_PROFILE=true
        fi
    fi
    
    if [ -f "$HOME/.zshrc" ]; then
        if ! grep -q "\.codebroker/bin" "$HOME/.zshrc"; then
            echo "" >> "$HOME/.zshrc"
            echo "# codebroker path" >> "$HOME/.zshrc"
            echo "$EXPORT_LINE" >> "$HOME/.zshrc"
            echo "Added codebroker to PATH in ~/.zshrc"
            ADDED_TO_PROFILE=true
        fi
    fi
    
    if [ "$ADDED_TO_PROFILE" = true ]; then
        echo "Please restart your terminal or run:"
        echo "  source ~/.bashrc  # (or ~/.zshrc for zsh)"
    else
        echo "Please add $BIN_DIR to your PATH."
        echo "For example, add this to your shell profile:"
        echo "  $EXPORT_LINE"
    fi
    echo "=========================================================="
fi

echo ""
echo "Installation complete! Run 'codebroker --help' to get started."
