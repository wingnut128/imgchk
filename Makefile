BINARY_NAME := imgchk
MODULE := imgchk
GO := go
GOFLAGS :=
LDFLAGS := -s -w

# Build output directory
DIST := dist

.PHONY: all build clean run fmt vet lint test install

all: build

## build: Build the binary
build:
	$(GO) build $(GOFLAGS) -ldflags "$(LDFLAGS)" -o $(BINARY_NAME) .

## dist: Build release binaries for all platforms
dist:
	@mkdir -p $(DIST)
	GOOS=darwin GOARCH=amd64 $(GO) build $(GOFLAGS) -ldflags "$(LDFLAGS)" -o $(DIST)/$(BINARY_NAME)-darwin-amd64 .
	GOOS=darwin GOARCH=arm64 $(GO) build $(GOFLAGS) -ldflags "$(LDFLAGS)" -o $(DIST)/$(BINARY_NAME)-darwin-arm64 .
	GOOS=linux GOARCH=amd64 $(GO) build $(GOFLAGS) -ldflags "$(LDFLAGS)" -o $(DIST)/$(BINARY_NAME)-linux-amd64 .
	GOOS=linux GOARCH=arm64 $(GO) build $(GOFLAGS) -ldflags "$(LDFLAGS)" -o $(DIST)/$(BINARY_NAME)-linux-arm64 .

## install: Install to $GOPATH/bin
install:
	$(GO) install $(GOFLAGS) -ldflags "$(LDFLAGS)" .

## run: Build and run with ARGS (e.g., make run ARGS="nginx.tar")
run: build
	./$(BINARY_NAME) $(ARGS)

## test: Run tests
test:
	$(GO) test ./... -v

## fmt: Format source code
fmt:
	$(GO) fmt ./...

## vet: Run go vet
vet:
	$(GO) vet ./...

## lint: Run staticcheck (install with: go install honnef.co/go/tools/cmd/staticcheck@latest)
lint: vet
	@command -v staticcheck >/dev/null 2>&1 && staticcheck ./... || echo "staticcheck not installed, skipping"

## tidy: Tidy module dependencies
tidy:
	$(GO) mod tidy

## clean: Remove build artifacts
clean:
	rm -f $(BINARY_NAME)
	rm -rf $(DIST)
	$(GO) clean

## help: Show this help
help:
	@echo "Usage: make [target]"
	@echo ""
	@sed -n 's/^## //p' $(MAKEFILE_LIST) | column -t -s ':' | sed 's/^/  /'
