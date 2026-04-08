BINARY_NAME := imgchk
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)

.PHONY: all build release clean run test fmt lint install help

all: build

## build: Build debug binary
build:
	cargo build

## release: Build optimized release binary
release:
	cargo build --release

## run: Build and run with ARGS (e.g., make run ARGS="nginx.tar")
run: build
	cargo run -- $(ARGS)

## test: Run tests
test:
	cargo test

## fmt: Format source code
fmt:
	cargo fmt

## lint: Run clippy lints
lint:
	cargo clippy -- -D warnings

## install: Install to ~/.cargo/bin
install:
	cargo install --path .

## clean: Remove build artifacts
clean:
	cargo clean

## help: Show this help
help:
	@echo "Usage: make [target]"
	@echo ""
	@sed -n 's/^## //p' $(MAKEFILE_LIST) | column -t -s ':' | sed 's/^/  /'
