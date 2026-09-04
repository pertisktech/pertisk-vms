.PHONY: release release-amd release-arm release-amd64 release-arm64 release-sbc

# make release-amd VERSION=0.1.0
# make release-arm VERSION=0.1.0
# make release-sbc BOARD=orangepi5plus VERSION=0.1.0
# Images: release/pertisk-node-$(VERSION)-amd64.raw
#         release/pertisk-node-$(VERSION)-arm64.raw
#         release/pertisk-node-$(VERSION)-$(BOARD).img.xz
GIT_VERSION := $(shell git describe --tags --always --dirty 2>/dev/null | sed 's/^v//')
CARGO_VERSION := $(shell awk '/^\[workspace.package\]/{p=1} p && /^version =/{gsub(/"/,"",$$3); print $$3; exit}' Cargo.toml)
VERSION ?= $(if $(GIT_VERSION),$(GIT_VERSION),$(or $(CARGO_VERSION),0.1.0))
BOARD ?= orangepi5plus

release: release-amd release-arm

release-amd release-amd64:
	./scripts/build-iso.sh amd64 $(VERSION)

release-arm release-arm64:
	./scripts/build-iso.sh arm64 $(VERSION)

# Board appliance (not the generic UEFI arm64.raw). Linux root + loop mounts.
release-sbc:
	sudo ./scripts/build-sbc-image.sh $(BOARD) $(VERSION)


# Delete a tag (local and remote).
delete-tag:
ifndef TAG
	$(error TAG is not set. Usage: make delete-tag TAG=0.1.10)
endif
	@echo "Deleting tag $(TAG)..."
	git tag -d $(TAG)
	git push origin -d $(TAG)

# Create a new tag.
create-tag:
ifndef TAG
	$(error TAG is not set. Usage: make create-tag TAG=0.1.10)
endif
	@echo "Creating tag $(TAG)..."
	git tag $(TAG)
	git push origin $(TAG)

# Delete and recreate a tag (force update). Use after amending a release commit.
# Usage: make retag TAG=0.1.10
retag:
ifndef TAG
	$(error TAG is not set. Usage: make retag TAG=0.1.10)
endif
	@echo "Recreating tag $(TAG)..."
	@echo "Deleting local tag (if exists)..."
	-git tag -d $(TAG) 2>/dev/null || true
	@echo "Deleting remote tag (if exists)..."
	-git push origin -d $(TAG) 2>/dev/null || true
	@echo "Creating new tag $(TAG)..."
	git tag $(TAG)
	@echo "Pushing tag $(TAG) to origin..."
	git push origin $(TAG)
	@echo "✓ Tag $(TAG) created and pushed successfully"

clean-tag: retag