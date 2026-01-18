.PHONY: all build clean linux-amd64 linux-amd64-v4 linux-arm64 darwin-amd64 darwin-arm64 help

BINARY_NAME=n2c
CMD_PATH=./cmd/n2c
BUILD_FLAGS=CGO_ENABLED=0
LDFLAGS=-ldflags="-s -w"

all: build

build:
	@echo "Building for current platform..."
	@$(BUILD_FLAGS) go build $(LDFLAGS) -o $(BINARY_NAME) $(CMD_PATH)
	@echo "Built: $(BINARY_NAME)"

help:
	@echo "Available targets:"
	@echo "  all/build       - Build for current platform (native)"
	@echo "  clean           - Remove all build artifacts"
	@echo ""
	@echo "Linux targets:"
	@echo "  linux-amd64     - Build for Linux AMD64 (GOAMD64=v3)"
	@echo "  linux-amd64-v4  - Build for Linux AMD64 (GOAMD64=v4)"
	@echo "  linux-arm64     - Build for Linux ARM64"
	@echo ""
	@echo "macOS targets:"
	@echo "  darwin-arm64    - Build for macOS ARM64 (Apple Silicon)"
	@echo "  darwin-amd64    - Build for macOS AMD64 (GOAMD64=v3)"

clean:
	@echo "Cleaning build artifacts..."
	@rm -f $(BINARY_NAME)
	@rm -f $(BINARY_NAME)-*
	@echo "Done."

linux-amd64:
	@echo "Building for Linux AMD64 (GOAMD64=v3)..."
	@$(BUILD_FLAGS) GOOS=linux GOARCH=amd64 GOAMD64=v3 go build $(LDFLAGS) -o $(BINARY_NAME)-linux-amd64 $(CMD_PATH)
	@echo "Built: $(BINARY_NAME)-linux-amd64"

linux-amd64-v4:
	@echo "Building for Linux AMD64 (GOAMD64=v4)..."
	@$(BUILD_FLAGS) GOOS=linux GOARCH=amd64 GOAMD64=v4 go build $(LDFLAGS) -o $(BINARY_NAME)-linux-amd64-v4 $(CMD_PATH)
	@echo "Built: $(BINARY_NAME)-linux-amd64-v4"

linux-arm64:
	@echo "Building for Linux ARM64..."
	@$(BUILD_FLAGS) GOOS=linux GOARCH=arm64 go build $(LDFLAGS) -o $(BINARY_NAME)-linux-arm64 $(CMD_PATH)
	@echo "Built: $(BINARY_NAME)-linux-arm64"

darwin-arm64:
	@echo "Building for macOS ARM64 (Apple Silicon)..."
	@$(BUILD_FLAGS) GOOS=darwin GOARCH=arm64 go build $(LDFLAGS) -o $(BINARY_NAME)-darwin-arm64 $(CMD_PATH)
	@echo "Built: $(BINARY_NAME)-darwin-arm64"

darwin-amd64:
	@echo "Building for macOS AMD64 (GOAMD64=v3)..."
	@$(BUILD_FLAGS) GOOS=darwin GOARCH=amd64 GOAMD64=v3 go build $(LDFLAGS) -o $(BINARY_NAME)-darwin-amd64 $(CMD_PATH)
	@echo "Built: $(BINARY_NAME)-darwin-amd64"
