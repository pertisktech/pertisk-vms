package provider

import (
	"net"
	"strings"
	"testing"
	"time"

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
	if got := disksToModel(vm, nil); len(got) != 0 {
		t.Fatalf("omitted disk blocks must stay empty, got %d", len(got))
	}
	prev := []diskModel{{Size: types.StringValue("32G"), Name: types.StringUnknown()}}
	got := disksToModel(vm, prev)
	if len(got) != 1 || got[0].VolumeID.ValueString() != "vol-1" || got[0].Size.ValueString() != "32G" {
		t.Fatalf("configured disk: %#v", got)
	}
	if got[0].Name.IsUnknown() {
		t.Fatal("name must be known after apply")
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

func TestGuestIPv4(t *testing.T) {
	if guestIPv4(nil) != "" {
		t.Fatal("nil vm")
	}
	vm := &client.VM{Spec: client.VMSpec{Nets: []client.Nic{{IP: " 10.1.1.169 "}}}}
	if got := guestIPv4(vm); got != "10.1.1.169" {
		t.Fatalf("got %q", got)
	}
}

func TestTcpOpen(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	go func() {
		c, err := ln.Accept()
		if err == nil {
			_ = c.Close()
		}
	}()
	_, port, err := net.SplitHostPort(ln.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	if !tcpOpen("127.0.0.1", port, time.Second) {
		t.Fatal("listener must be reachable")
	}
	if tcpOpen("127.0.0.1", "1", 200*time.Millisecond) {
		t.Fatal("closed port must not look reachable")
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

func TestKeepPlannedBlocksPreservesStarted(t *testing.T) {
	plan := vmModel{
		Started: types.BoolValue(true),
		Disks:   types.ListValueMust(diskObjectType, []attr.Value{}),
		Nics:    types.ListValueMust(nicObjectType, []attr.Value{}),
	}
	vm := &client.VM{
		ID:    "105",
		Spec:  client.VMSpec{Name: "rocky-1", ConsoleType: "serial"},
		State: "created",
	}
	state := keepPlannedBlocks(plan, vmToModel(t.Context(), vm, plan))
	if !state.Started.ValueBool() {
		t.Fatal("planned started=true must remain true after apply")
	}
	if state.State.ValueString() != "created" {
		t.Fatalf("actual state should stay created, got %s", state.State.ValueString())
	}
}

func TestGuestMatchesPlan(t *testing.T) {
	plan := vmModel{Name: types.StringValue("rocky-1")}
	ok := &client.VM{ID: "105", Spec: client.VMSpec{Name: "rocky-1"}}
	if err := guestMatchesPlan(plan, ok); err != nil {
		t.Fatal(err)
	}
	err := guestMatchesPlan(plan, &client.VM{ID: "105", Spec: client.VMSpec{Name: "almalinux-2"}})
	if err == nil || !strings.Contains(err.Error(), "almalinux-2") {
		t.Fatalf("got %v", err)
	}
}
