.PHONY: release release-amd release-arm release-amd64 release-arm64

# make release-amd VERSION=0.1.0
# make release-arm VERSION=0.1.0
# Images: release/pertisk-node-$(VERSION)-amd64.raw
#         release/pertisk-node-$(VERSION)-arm64.raw
GIT_VERSION := $(shell git describe --tags --always --dirty 2>/dev/null | sed 's/^v//')
CARGO_VERSION := $(shell awk '/^\[workspace.package\]/{p=1} p && /^version =/{gsub(/"/,"",$$3); print $$3; exit}' Cargo.toml)
VERSION ?= $(if $(GIT_VERSION),$(GIT_VERSION),$(or $(CARGO_VERSION),0.1.0))

release: release-amd release-arm

release-amd release-amd64:
	./scripts/build-iso.sh amd64 $(VERSION)

release-arm release-arm64:
	./scripts/build-iso.sh arm64 $(VERSION)
