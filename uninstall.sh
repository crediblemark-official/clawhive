#!/usr/bin/env bash
# ClawHive OS uninstaller (Linux, macOS, WSL)
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/crediblemark-official/clawhive/master/uninstall.sh | sh

set -e

BINARY="clawhive"
INSTALL_DIR="${CLAWHIVE_INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="$HOME/.clawhive"

echo "ClawHive OS Uninstaller"
echo "========================"
echo ""
echo "This will remove:"
echo "  - Binary: $INSTALL_DIR/$BINARY"
echo "  - Config & data: $CONFIG_DIR"
echo "  - PATH entry from shell config (if added by installer)"
echo ""

# Non-interactive mode: set CLAWHIVE_UNINSTALL_FORCE=1 to skip confirmation
if [ -z "$CLAWHIVE_UNINSTALL_FORCE" ]; then
    printf "Are you sure you want to uninstall ClawHive? [y/N] "
    read -r response
    case "$response" in
        [yY][eE][sS]|[yY]) ;;
        *) echo "Uninstall cancelled."; exit 0 ;;
    esac
fi

# Remove binary
if [ -f "$INSTALL_DIR/$BINARY" ]; then
    rm -f "$INSTALL_DIR/$BINARY"
    echo "Removed: $INSTALL_DIR/$BINARY"
else
    echo "Binary not found: $INSTALL_DIR/$BINARY"
fi

# Remove config and data directory
if [ -d "$CONFIG_DIR" ]; then
    rm -rf "$CONFIG_DIR"
    echo "Removed: $CONFIG_DIR"
else
    echo "Config directory not found: $CONFIG_DIR"
fi

# Remove PATH export from shell config files
SHELL_NAME=$(basename "$SHELL")
case "$SHELL_NAME" in
    bash)   RC_FILES=("$HOME/.bashrc") ;;
    zsh)    RC_FILES=("$HOME/.zshrc") ;;
    fish)   RC_FILES=("$HOME/.config/fish/config.fish") ;;
    *)      RC_FILES=("$HOME/.bashrc" "$HOME/.zshrc") ;;
esac

for rc in "${RC_FILES[@]}"; do
    if [ -f "$rc" ] && grep -q "export PATH=\"$INSTALL_DIR:\\$PATH\"" "$rc" 2>/dev/null; then
        sed -i.bak "/export PATH=\"$INSTALL_DIR:\\\$PATH\"/d" "$rc"
        rm -f "$rc.bak"
        echo "Updated: $rc"
    fi
done

echo ""
echo "ClawHive has been uninstalled."
echo "You may need to restart your terminal for PATH changes to take effect."
