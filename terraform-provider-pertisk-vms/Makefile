VERSION ?= 0.1.0
OS_ARCH := $(shell go env GOOS)_$(shell go env GOARCH)
BIN := terraform-provider-pertisk-vms
PLUGIN_DIR := $(HOME)/.terraform.d/plugins/registry.terraform.io/pertisktech/pertisk-vms/$(VERSION)/$(OS_ARCH)

.PHONY: build install tidy test fmt

tidy:
	go mod tidy

build: tidy
	go build -ldflags="-X main.version=$(VERSION)" -o bin/$(BIN) .

install: build
	mkdir -p "$(PLUGIN_DIR)"
	cp bin/$(BIN) "$(PLUGIN_DIR)/$(BIN)_v$(VERSION)"
	@echo "installed → $(PLUGIN_DIR)/$(BIN)_v$(VERSION)"

fmt:
	gofmt -w .

test:
	go test ./...
