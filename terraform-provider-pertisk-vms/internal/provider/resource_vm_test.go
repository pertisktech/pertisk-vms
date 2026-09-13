package provider

import (
	"testing"

	"github.com/hashicorp/terraform-plugin-framework/attr"
	"github.com/hashicorp/terraform-plugin-framework/types"

	"github.com/pertisktech/pertisk-vms/terraform-provider-pertisk-vms/internal/client"
)

func TestDisksToModelFromClone(t *testing.T) {
	vm := &client.VM{
		Spec: client.VMSpec{
			Disks: []client.Disk{
				{VolumeID: "vol-1"},
				{Cdrom: true, ISOName: "cidata.iso"},
			},
		},
	}
	got := disksToModel(vm, nil)
	if len(got) != 1 {
		t.Fatalf("len=%d", len(got))
	}
	if got[0].VolumeID.ValueString() != "vol-1" {
		t.Fatalf("volume_id=%s", got[0].VolumeID.ValueString())
	}
}

func TestListKnownEmpty(t *testing.T) {
	if !listKnownEmpty(types.ListNull(diskObjectType)) {
		t.Fatal("null should be known-empty")
	}
	if !listKnownEmpty(types.ListValueMust(diskObjectType, []attr.Value{})) {
		t.Fatal("empty list should be known-empty")
	}
	if listKnownEmpty(types.ListUnknown(diskObjectType)) {
		t.Fatal("unknown should not be known-empty")
	}
}

func TestPickDefaultNetworkPrefersNAT(t *testing.T) {
	nets := []client.Network{
		{ID: "br1", Name: "uplink", Mode: "bridge"},
		{ID: "nat1", Name: "lan", Mode: "nat"},
	}
	if got := pickDefaultNetworkID(nets); got != "nat1" {
		t.Fatalf("got %s", got)
	}
	if got := pickDefaultNetworkID(nil); got != "" {
		t.Fatalf("empty=%s", got)
	}
}

func TestKeepPlannedBlocksDropsCloneDisks(t *testing.T) {
	plan := vmModel{
		Disks: types.ListValueMust(diskObjectType, []attr.Value{}),
		Nics:  types.ListValueMust(nicObjectType, []attr.Value{}),
	}
	vm := &client.VM{
		ID: "110",
		Spec: client.VMSpec{
			Name:      "ubuntu-1",
			Disks:     []client.Disk{{VolumeID: "vol-1"}},
			ConsoleType: "serial",
		},
		State: "running",
	}
	state := keepPlannedBlocks(plan, vmToModel(t.Context(), vm, plan))
	if !listKnownEmpty(state.Disks) {
		t.Fatalf("create/update must not add disk blocks: %#v", state.Disks)
	}
}
