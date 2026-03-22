#!/usr/bin/env bash
# chmod +x install.sh
set -e

BINARY_NAME="claude-client"
INSTALL_DIR="$HOME/.local/bin"
CONFIG_DIR="$HOME/.config/claude-client"
SERVICE_DIR="$HOME/.config/systemd/user"
AUTOSTART_DIR="$HOME/.config/autostart"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "Installing Claude Client..."

# Tear down existing service before replacing it
if systemctl --user is-active --quiet claude-client 2>/dev/null; then
    echo "  Stopping running claude-client service..."
    systemctl --user stop claude-client
fi
if systemctl --user is-enabled --quiet claude-client 2>/dev/null; then
    echo "  Disabling existing claude-client service..."
    systemctl --user disable claude-client
fi

# Build
cargo build --release -p claude-client
mkdir -p "$INSTALL_DIR"
cp "$(cargo metadata --format-version=1 | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")/release/$BINARY_NAME" "$INSTALL_DIR/"
echo "✓ Binary installed to $INSTALL_DIR/$BINARY_NAME"

# Config dir + env template
mkdir -p "$CONFIG_DIR"
if [ ! -f "$CONFIG_DIR/env" ]; then
    cp "$SCRIPT_DIR/../.env.client.example" "$CONFIG_DIR/env"
    echo "✓ Created $CONFIG_DIR/env — edit this with your settings"
else
    echo "  (skipped env file — already exists at $CONFIG_DIR/env)"
fi

# Systemd user service
mkdir -p "$SERVICE_DIR"

# Install updated unit file and re-enable
sed "s|%h|$HOME|g" "$SCRIPT_DIR/claude-client.service" > "$SERVICE_DIR/claude-client.service"
systemctl --user daemon-reload
systemctl --user enable claude-client
echo "✓ systemd user service installed and enabled"
echo "  Start now with: systemctl --user start claude-client"

echo ""
echo "Done! Edit $CONFIG_DIR/env then:"
echo "  systemctl --user start claude-client"
echo "  systemctl --user status claude-client"
