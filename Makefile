PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
BINARY := target/release/speak-mcp
APP_DIR := /Applications/SpeakConfig.app
CONFIG_DIR ?= $(HOME)/.config/speak-mcp
CONFIG_FILE ?= $(CONFIG_DIR)/config.json

.DEFAULT_GOAL := help

.PHONY: help build build-app config install install-mcp install-app

help:
	@printf '%s\n' 'Available targets:'
	@printf '%s\n' '  make build    Build the release speak-mcp binary.'
	@printf '%s\n' '  make config   Create the default locale voice configuration.'
	@printf '%s\n' '  make build-app    Build and package SpeakConfig.app.'
	@printf '%s\n' '  make install-mcp  Overwrite $(BINDIR)/speak-mcp with the release binary.'
	@printf '%s\n' '  make install-app  Overwrite $(APP_DIR).'
	@printf '%s\n' '  make install      Install both speak-mcp and SpeakConfig.app.'

build:
	cargo build --release

build-app:
	cargo build --release --manifest-path speak-config/Cargo.toml
	cd speak-config && bash package_app.sh

config:
	@test ! -e "$(CONFIG_FILE)" || { echo "Config already exists: $(CONFIG_FILE)"; exit 1; }
	install -d "$(CONFIG_DIR)"
	@printf '%s\n' '{' \
		'  "voicevox_default_speaker": null,' \
		'  "aivis_default_speaker": null,' \
		'  "locale": {' \
		'    "en_US": "Nathan (Enhanced)",' \
		'    "en_AU": "Karen (Premium)",' \
		'    "en_UK": "Jamie (Enhanced)",' \
		'    "ko_KR": "Yuna (Premium)"' \
		'  }' \
		'}' > "$(CONFIG_FILE)"

install: install-mcp install-app

install-mcp: build
	install -d "$(BINDIR)"
	install -m 755 "$(BINARY)" "$(BINDIR)/speak-mcp"

install-app: build-app
	ditto "speak-config/SpeakConfig.app" "$(APP_DIR)"
