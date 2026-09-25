PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
BINARY := target/release/speak-mcp

.DEFAULT_GOAL := help

.PHONY: help build install

help:
	@printf '%s\n' 'Available targets:'
	@printf '%s\n' '  make build    Build the release speak-mcp binary.'
	@printf '%s\n' '  make install  Install the release binary to $(BINDIR)/speak-mcp.'

build:
	cargo build --release

install: build
	install -d "$(BINDIR)"
	install -m 755 "$(BINARY)" "$(BINDIR)/speak-mcp"
