#!/bin/bash
# This installer is intentionally disabled. Installation and MCP client configuration
# must be performed explicitly by the user, not by this repository script.
: <<'INSTALL_DISABLED'
# ==========================================
# speak-mcp Installer (English)
# ==========================================
set -e

INSTALL_DIR="$HOME/.local/bin"
CONFIG_DIR="$HOME/.config/speak-mcp"
CONFIG_PATH="$CONFIG_DIR/config.json"
BINARY_NAME="speak-mcp"
BINARY_PATH="$INSTALL_DIR/$BINARY_NAME"

echo "======================================"
echo " speak-mcp Installer"
echo "======================================"
echo "Destination: $INSTALL_DIR"
echo ""

# ------------------------------------------
# 1. Build or copy pre-built binary
# ------------------------------------------
IS_RELEASE_ZIP=false

if [ -f "./$BINARY_NAME" ]; then
    echo "[INFO] Pre-built binary detected. Skipping build."
    IS_RELEASE_ZIP=true
else
    echo "[INFO] Source detected. Starting build..."

    if ! command -v cargo &> /dev/null; then
        echo "[ERROR] cargo not found."
        echo "  Please download the binary ZIP from the Releases page, or"
        echo "  install Rust and try again: https://rustup.rs"
        exit 1
    fi

    echo "[INFO] Building speak-mcp server..."
    cargo build --release

    echo "[INFO] Building speak-config tool..."
    if [ -d "speak-config" ]; then
        cd speak-config
        cargo build --release
        if [ -f "package_app.sh" ]; then
            ./package_app.sh
        fi
        cd ..
    fi
fi

# ------------------------------------------
# 2. Copy files to install directory
# ------------------------------------------
mkdir -p "$INSTALL_DIR"

# speak-mcp binary
if [ "$IS_RELEASE_ZIP" = true ]; then
    cp "./$BINARY_NAME" "$INSTALL_DIR/"
elif [ -f "target/release/$BINARY_NAME" ]; then
    cp "target/release/$BINARY_NAME" "$INSTALL_DIR/"
else
    echo "[ERROR] speak-mcp binary not found."
    exit 1
fi
chmod +x "$BINARY_PATH"

# SpeakConfig.app
if [ -d "./SpeakConfig.app" ]; then
    rm -rf "$INSTALL_DIR/SpeakConfig.app"
    cp -r "./SpeakConfig.app" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/SpeakConfig.app/Contents/MacOS/SpeakConfig"
elif [ -d "speak-config/SpeakConfig.app" ]; then
    rm -rf "$INSTALL_DIR/SpeakConfig.app"
    cp -r "speak-config/SpeakConfig.app" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/SpeakConfig.app/Contents/MacOS/SpeakConfig"
elif [ -f "speak-config/target/release/speak-config" ]; then
    cp "speak-config/target/release/speak-config" "$INSTALL_DIR/"
fi

# Setup scripts
if [ -d "./setup" ]; then
    cp -r "./setup" "$INSTALL_DIR/"
fi

# config.json (do not overwrite existing config)
mkdir -p "$CONFIG_DIR"
if [ ! -f "$CONFIG_PATH" ]; then
    echo "[INFO] Creating default config.json..."
    cat > "$CONFIG_PATH" <<EOF
{
  "voicevox_default_speaker": null,
  "aivis_default_speaker": null,
  "rate": 200,
  "locale": {
    "en_US": "Nathan (Enhanced)",
    "en_AU": "Karen (Premium)",
    "en_UK": "Jamie (Enhanced)",
    "ko_KR": "Yuna (Premium)"
  }
}
EOF
fi

echo ""
echo "[OK] Files installed to: $INSTALL_DIR"
echo ""

# ------------------------------------------
# 3. MCP client configuration
# ------------------------------------------
echo "======================================"
echo " MCP Client Configuration"
echo "======================================"
echo ""
echo "[INFO] This installer does not modify MCP client or agent configuration files."
echo "  Register speak-mcp manually in the MCP client you want to use:"
echo ""
echo '  "mcpServers": {'
echo '    "speak": {'
echo "      \"command\": \"$BINARY_PATH\""
echo '    }'
echo '  }'
echo ""

# ------------------------------------------
# 4. Done
# ------------------------------------------
echo "======================================"
echo " Installation complete!"
echo "======================================"
echo ""
echo "Binary : $BINARY_PATH"
echo "Config : $CONFIG_PATH"
if [ -d "$INSTALL_DIR/SpeakConfig.app" ]; then
    echo ""
    echo "To change the default voice, open SpeakConfig.app:"
    echo "  open \"$INSTALL_DIR/SpeakConfig.app\""
fi
echo ""
INSTALL_DISABLED
