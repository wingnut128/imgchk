binary_name := "imgchk"
version := `git describe --tags --always --dirty 2>/dev/null || echo dev`

# Show available recipes
default:
    @just --list

# Build debug binary
build:
    cargo build && echo "pass" > target/.last_build_status || (echo "fail" > target/.last_build_status; exit 1)

# Build optimized release binary
release:
    cargo build --release && echo "pass" > target/.last_build_status || (echo "fail" > target/.last_build_status; exit 1)

# Build and run with args (e.g. `just run nginx.tar`)
run *args: build
    cargo run -- {{args}}

# Run tests
test:
    cargo test

# Format source code
fmt:
    cargo fmt

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Install to ~/.cargo/bin
install:
    cargo install --path .

# Install git pre-commit hook
hooks:
    cp scripts/pre-commit .git/hooks/pre-commit
    chmod +x .git/hooks/pre-commit
    @echo "Pre-commit hook installed."

# Remove build artifacts
clean:
    cargo clean
