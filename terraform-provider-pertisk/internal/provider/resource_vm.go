package provider

import (
	"context"
	"fmt"
	"strings"

	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/booldefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/int64default"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/listplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/objectplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringdefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"
	"github.com/hashicorp/terraform-plugin-log/tflog"

	"github.com/pertisktech/pertisk-vms/terraform-provider-pertisk/internal/client"
)

var (
	_ resource.Resource                = &vmResource{}
	_ resource.ResourceWithConfigure   = &vmResource{}
	_ resource.ResourceWithImportState = &vmResource{}
	_ resource.ResourceWithModifyPlan  = &vmResource{}
)

func NewVMResource() resource.Resource { return &vmResource{} }

type vmResource struct {
	api *client.Client
}

type diskModel struct {
	Size     types.String `tfsdk:"size"`
	Name     types.String `tfsdk:"name"`
	Format   types.String `tfsdk:"format"`
	VolumeID types.String `tfsdk:"volume_id"`
}

type nicModel struct {
	NetworkID types.String `tfsdk:"network_id"`
	IP        types.String `tfsdk:"ip"`
	Tap       types.String `tfsdk:"tap"`
	MAC       types.String `tfsdk:"mac"`
}

type cloneModel struct {
	ID         types.String `tfsdk:"id"`
	TemplateID types.String `tfsdk:"template_id"`
	Name       types.String `tfsdk:"name"`
	Linked     types.Bool   `tfsdk:"linked"`
	DiskSize   types.String `tfsdk:"disk_size"`
}

type cloudInitModel struct {
	Hostname types.String `tfsdk:"hostname"`
	User     types.String `tfsdk:"user"`
	Password types.String `tfsdk:"password"`
	SSHKeys  types.List   `tfsdk:"ssh_keys"`
	Userdata types.String `tfsdk:"userdata"`
}

type vmModel struct {
	ID             types.String    `tfsdk:"id"`
	VmID           types.String    `tfsdk:"vm_id"`
	Name           types.String    `tfsdk:"name"`
	VCPUs          types.Int64     `tfsdk:"vcpus"`
	MemoryMiB      types.Int64     `tfsdk:"memory_mib"`
	HA             types.Bool      `tfsdk:"ha"`
	Autostart      types.Bool      `tfsdk:"autostart"`
	AutostartDelay types.Int64     `tfsdk:"autostart_delay"`
	AutostartOrder types.Int64     `tfsdk:"autostart_order"`
	ConsoleType    types.String    `tfsdk:"console_type"`
	Started        types.Bool      `tfsdk:"started"`
	ISO            types.String    `tfsdk:"iso"`
	State          types.String    `tfsdk:"state"`
	NodeID         types.String    `tfsdk:"node_id"`
	Disks          []diskModel     `tfsdk:"disk"`
	Nics           []nicModel      `tfsdk:"nic"`
	Clone          *cloneModel     `tfsdk:"clone"`
	CloudInit      *cloudInitModel `tfsdk:"cloud_init"`
}

func (r *vmResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_vm"
}

func (r *vmResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "A Pertisk guest. Create from scratch (disk + optional ISO) or clone a cloud template.",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				Computed:            true,
				MarkdownDescription: "Guest ID (same as `vm_id`).",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"vm_id": schema.StringAttribute{
				Optional:            true,
				Computed:            true,
				MarkdownDescription: "Numeric guest ID (3–10 digits). Assigned by the API when omitted.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplaceIfConfigured(),
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"name": schema.StringAttribute{
				Required: true,
			},
			"vcpus": schema.Int64Attribute{
				Optional: true,
				Computed: true,
				Default:  int64default.StaticInt64(1),
			},
			"memory_mib": schema.Int64Attribute{
				Optional: true,
				Computed: true,
				Default:  int64default.StaticInt64(512),
			},
			"ha": schema.BoolAttribute{
				Optional: true,
				Computed: true,
				Default:  booldefault.StaticBool(true),
			},
			"autostart": schema.BoolAttribute{
				Optional: true,
				Computed: true,
				Default:  booldefault.StaticBool(false),
			},
			"autostart_delay": schema.Int64Attribute{
				Optional: true,
				Computed: true,
				Default:  int64default.StaticInt64(0),
			},
			"autostart_order": schema.Int64Attribute{
				Optional: true,
				Computed: true,
				Default:  int64default.StaticInt64(0),
			},
			"console_type": schema.StringAttribute{
				Optional:            true,
				Computed:            true,
				Default:             stringdefault.StaticString("serial"),
				MarkdownDescription: "`serial` or `graphics`.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"started": schema.BoolAttribute{
				Optional:            true,
				Computed:            true,
				Default:             booldefault.StaticBool(false),
				MarkdownDescription: "Whether the guest should be running.",
			},
			"iso": schema.StringAttribute{
				Optional:            true,
				MarkdownDescription: "ISO library name to attach as CD-ROM.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"state": schema.StringAttribute{
				Computed: true,
			},
			"node_id": schema.StringAttribute{
				Computed: true,
			},
		},
		Blocks: map[string]schema.Block{
			"disk": schema.ListNestedBlock{
				MarkdownDescription: "Disks to create or attach. Omit when cloning — cloned disks are stored in state.",
				PlanModifiers: []planmodifier.List{
					listplanmodifier.RequiresReplace(),
				},
				NestedObject: schema.NestedBlockObject{
					Attributes: map[string]schema.Attribute{
						"size": schema.StringAttribute{
							Optional:            true,
							Computed:            true,
							MarkdownDescription: "Create a new volume of this size (e.g. `32G`) when `volume_id` is unset.",
						},
						"name": schema.StringAttribute{
							Optional: true,
							Computed: true,
						},
						"format": schema.StringAttribute{
							Optional: true,
							Computed: true,
							Default:  stringdefault.StaticString("qcow2"),
						},
						"volume_id": schema.StringAttribute{
							Optional: true,
							Computed: true,
							PlanModifiers: []planmodifier.String{
								stringplanmodifier.UseStateForUnknown(),
							},
						},
					},
				},
			},
			"nic": schema.ListNestedBlock{
				MarkdownDescription: "Guest NICs. On clone, the first NIC is passed to the clone API.",
				PlanModifiers: []planmodifier.List{
					listplanmodifier.RequiresReplace(),
				},
				NestedObject: schema.NestedBlockObject{
					Attributes: map[string]schema.Attribute{
						"network_id": schema.StringAttribute{Required: true},
						"ip":         schema.StringAttribute{Optional: true},
						"tap":        schema.StringAttribute{Computed: true},
						"mac":        schema.StringAttribute{Computed: true},
					},
				},
			},
			"clone": schema.SingleNestedBlock{
				MarkdownDescription: "Clone a cloud template or stopped guest instead of defining a new VM.",
				PlanModifiers: []planmodifier.Object{
					objectplanmodifier.RequiresReplace(),
				},
				Attributes: map[string]schema.Attribute{
					"template_id": schema.StringAttribute{
						Optional:            true,
						MarkdownDescription: "Template ID (`pertisk_template.ubuntu.id`). Preferred over `id`/`name`.",
					},
					"id": schema.StringAttribute{
						Optional:            true,
						MarkdownDescription: "Source guest or template ID.",
					},
					"name": schema.StringAttribute{
						Optional:            true,
						MarkdownDescription: "Source template or guest name (used when `template_id` and `id` are unset).",
					},
					"linked": schema.BoolAttribute{
						Optional: true,
						Computed: true,
						Default:  booldefault.StaticBool(false),
					},
					"disk_size": schema.StringAttribute{
						Optional:            true,
						MarkdownDescription: "Grow the first cloned disk, e.g. `40G`.",
					},
				},
			},
			"cloud_init": schema.SingleNestedBlock{
				MarkdownDescription: "Cloud-init identity. Sent with clone, or attached as a cidata ISO on a new guest.",
				PlanModifiers: []planmodifier.Object{
					objectplanmodifier.RequiresReplace(),
				},
				Attributes: map[string]schema.Attribute{
					"hostname": schema.StringAttribute{Optional: true},
					"user":     schema.StringAttribute{Optional: true},
					"password": schema.StringAttribute{Optional: true, Sensitive: true},
					"ssh_keys": schema.ListAttribute{
						Optional:    true,
						ElementType: types.StringType,
					},
					"userdata": schema.StringAttribute{Optional: true},
				},
			},
		},
	}
}

func (r *vmResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	r.api = configureClient(req.ProviderData, &resp.Diagnostics)
}

func (r *vmResource) ModifyPlan(ctx context.Context, req resource.ModifyPlanRequest, resp *resource.ModifyPlanResponse) {
	if req.Plan.Raw.IsNull() || req.State.Raw.IsNull() {
		return
	}
	var config, state, plan vmModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &config)...)
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if len(config.Disks) == 0 && len(state.Disks) > 0 {
		plan.Disks = state.Disks
	}
	if len(config.Nics) == 0 && len(state.Nics) > 0 {
		plan.Nics = state.Nics
	}
	if config.Clone == nil && state.Clone != nil {
		plan.Clone = state.Clone
	}
	if config.CloudInit == nil && state.CloudInit != nil {
		plan.CloudInit = state.CloudInit
	}
	resp.Diagnostics.Append(resp.Plan.Set(ctx, &plan)...)
}

func (r *vmResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan vmModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	var vm *client.VM
	var err error
	if cloneConfigured(plan.Clone) {
		vm, err = r.clone(ctx, plan)
	} else {
		vm, err = r.define(ctx, plan)
	}
	if err != nil {
		resp.Diagnostics.AddError("Create guest failed", err.Error())
		return
	}
	if plan.Started.ValueBool() && vm.State != "running" {
		started, startErr := r.api.StartVM(vm.ID.String())
		if startErr != nil {
			resp.Diagnostics.AddWarning("Guest defined but start failed", startErr.Error())
		} else {
			vm = started
		}
	}
	state := vmToModel(vm, plan)
	resp.Diagnostics.Append(resp.State.Set(ctx, &state)...)
}

func (r *vmResource) clone(ctx context.Context, plan vmModel) (*client.VM, error) {
	src := strings.TrimSpace(plan.Clone.TemplateID.ValueString())
	if src == "" {
		src = strings.TrimSpace(plan.Clone.ID.ValueString())
	}
	if src == "" {
		src = strings.TrimSpace(plan.Clone.Name.ValueString())
	}
	if src == "" {
		return nil, fmt.Errorf("clone.template_id, clone.id, or clone.name is required")
	}
	tpl, err := r.api.FindTemplate(src)
	if err != nil {
		found, vmErr := r.api.FindVM(src)
		if vmErr != nil {
			return nil, err
		}
		tpl = found
	}
	body := client.CloneVMRequest{
		Name:   plan.Name.ValueString(),
		Linked: plan.Clone.Linked.ValueBool(),
		HA:     boolPtr(plan.HA.ValueBool()),
		Start:  false,
	}
	if !plan.VmID.IsNull() && !plan.VmID.IsUnknown() && plan.VmID.ValueString() != "" {
		body.ID = client.NumericID(plan.VmID.ValueString())
	}
	vcpus := int(plan.VCPUs.ValueInt64())
	mem := int(plan.MemoryMiB.ValueInt64())
	body.VCPUs = &vcpus
	body.MemoryMiB = &mem
	body.Autostart = boolPtr(plan.Autostart.ValueBool())
	if len(plan.Nics) > 0 {
		body.NetworkID = plan.Nics[0].NetworkID.ValueString()
		body.IP = plan.Nics[0].IP.ValueString()
	}
	if !plan.Clone.DiskSize.IsNull() && plan.Clone.DiskSize.ValueString() != "" {
		n, err := client.ParseSize(plan.Clone.DiskSize.ValueString())
		if err != nil {
			return nil, err
		}
		body.DiskSizeBytes = &n
	}
	if cloudInitConfigured(plan.CloudInit) {
		ci, err := cloudInitFromModel(plan.CloudInit)
		if err != nil {
			return nil, err
		}
		if ci.Hostname == "" {
			ci.Hostname = plan.Name.ValueString()
		}
		body.CloudInit = ci
	}
	tflog.Info(ctx, "cloning guest", map[string]any{"source": tpl.ID.String(), "name": body.Name})
	vm, err := r.api.CloneVM(tpl.ID.String(), body)
	if err != nil {
		return nil, err
	}
	for i, nic := range plan.Nics {
		if i == 0 {
			continue
		}
		if _, err := r.api.AttachNic(vm.ID.String(), nic.NetworkID.ValueString(), nic.IP.ValueString()); err != nil {
			_ = r.api.DeleteVM(vm.ID.String())
			return nil, err
		}
	}
	return r.api.GetVM(vm.ID.String())
}

func (r *vmResource) define(ctx context.Context, plan vmModel) (*client.VM, error) {
	body := client.CreateVMRequest{
		Name:           plan.Name.ValueString(),
		VCPUs:          int(plan.VCPUs.ValueInt64()),
		MemoryMiB:      int(plan.MemoryMiB.ValueInt64()),
		HA:             plan.HA.ValueBool(),
		Autostart:      plan.Autostart.ValueBool(),
		AutostartDelay: uint64(plan.AutostartDelay.ValueInt64()),
		AutostartOrder: uint32(plan.AutostartOrder.ValueInt64()),
		ConsoleType:    plan.ConsoleType.ValueString(),
	}
	if !plan.VmID.IsNull() && !plan.VmID.IsUnknown() && plan.VmID.ValueString() != "" {
		body.ID = client.NumericID(plan.VmID.ValueString())
	}
	tflog.Info(ctx, "defining guest", map[string]any{"name": body.Name})
	vm, err := r.api.CreateVM(body)
	if err != nil {
		return nil, err
	}
	id := vm.ID.String()
	rollback := func() { _ = r.api.DeleteVM(id) }
	for i, disk := range plan.Disks {
		volID := disk.VolumeID.ValueString()
		if volID == "" {
			size := disk.Size.ValueString()
			if size == "" {
				rollback()
				return nil, fmt.Errorf("disk[%d]: size or volume_id is required", i)
			}
			bytes, err := client.ParseSize(size)
			if err != nil {
				rollback()
				return nil, err
			}
			name := disk.Name.ValueString()
			if name == "" {
				name = fmt.Sprintf("%s-disk", plan.Name.ValueString())
				if i > 0 {
					name = fmt.Sprintf("%s-disk-%d", plan.Name.ValueString(), i+1)
				}
			}
			format := disk.Format.ValueString()
			if format == "" {
				format = "qcow2"
			}
			vol, err := r.api.CreateVolume(client.CreateVolumeRequest{Name: name, SizeBytes: bytes, Format: format})
			if err != nil {
				rollback()
				return nil, err
			}
			volID = vol.ID
		}
		if _, err := r.api.AttachDisk(id, volID); err != nil {
			rollback()
			return nil, err
		}
	}
	if iso := plan.ISO.ValueString(); iso != "" {
		if _, err := r.api.AttachISO(id, iso); err != nil {
			rollback()
			return nil, err
		}
	}
	if cloudInitConfigured(plan.CloudInit) {
		ci, err := cloudInitFromModel(plan.CloudInit)
		if err != nil {
			rollback()
			return nil, err
		}
		hostname := ci.Hostname
		if hostname == "" {
			hostname = plan.Name.ValueString()
		}
		seed, err := r.api.CreateCloudInitISO(client.CloudInitISORequest{
			Name:     hostname + "-cidata.iso",
			Hostname: hostname,
			User:     ci.User,
			Password: ci.Password,
			SSHKeys:  ci.SSHKeys,
			Userdata: ci.Userdata,
		})
		if err != nil {
			rollback()
			return nil, err
		}
		if _, err := r.api.AttachISO(id, seed.Name); err != nil {
			rollback()
			return nil, err
		}
	}
	for _, nic := range plan.Nics {
		if _, err := r.api.AttachNic(id, nic.NetworkID.ValueString(), nic.IP.ValueString()); err != nil {
			rollback()
			return nil, err
		}
	}
	return r.api.GetVM(id)
}

func (r *vmResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state vmModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	vm, err := r.api.GetVM(state.ID.ValueString())
	if client.IsNotFound(err) {
		resp.State.RemoveResource(ctx)
		return
	}
	if err != nil {
		resp.Diagnostics.AddError("Read guest failed", err.Error())
		return
	}
	next := vmToModel(vm, state)
	resp.Diagnostics.Append(resp.State.Set(ctx, &next)...)
}

func (r *vmResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan, state vmModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	id := state.ID.ValueString()
	patch := client.UpdateVMRequest{}
	if plan.Name.ValueString() != state.Name.ValueString() {
		name := plan.Name.ValueString()
		patch.Name = &name
	}
	if plan.VCPUs.ValueInt64() != state.VCPUs.ValueInt64() {
		patch.VCPUs = intPtr(int(plan.VCPUs.ValueInt64()))
	}
	if plan.MemoryMiB.ValueInt64() != state.MemoryMiB.ValueInt64() {
		patch.MemoryMiB = intPtr(int(plan.MemoryMiB.ValueInt64()))
	}
	if plan.HA.ValueBool() != state.HA.ValueBool() {
		patch.HA = boolPtr(plan.HA.ValueBool())
	}
	if plan.Autostart.ValueBool() != state.Autostart.ValueBool() {
		patch.Autostart = boolPtr(plan.Autostart.ValueBool())
	}
	if plan.AutostartDelay.ValueInt64() != state.AutostartDelay.ValueInt64() {
		v := uint64(plan.AutostartDelay.ValueInt64())
		patch.AutostartDelay = &v
	}
	if plan.AutostartOrder.ValueInt64() != state.AutostartOrder.ValueInt64() {
		v := uint32(plan.AutostartOrder.ValueInt64())
		patch.AutostartOrder = &v
	}
	vm, err := r.api.UpdateVM(id, patch)
	if err != nil {
		resp.Diagnostics.AddError("Update guest failed", err.Error())
		return
	}
	if plan.Started.ValueBool() && vm.State != "running" {
		started, err := r.api.StartVM(id)
		if err != nil {
			resp.Diagnostics.AddError("Start guest failed", err.Error())
			return
		}
		vm = started
	}
	if !plan.Started.ValueBool() && vm.State == "running" {
		stopped, err := r.api.StopVM(id)
		if err != nil {
			resp.Diagnostics.AddError("Stop guest failed", err.Error())
			return
		}
		vm = stopped
	}
	next := vmToModel(vm, plan)
	resp.Diagnostics.Append(resp.State.Set(ctx, &next)...)
}

func (r *vmResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state vmModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if err := r.api.DeleteVM(state.ID.ValueString()); err != nil && !client.IsNotFound(err) {
		resp.Diagnostics.AddError("Delete guest failed", err.Error())
	}
}

func (r *vmResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("id"), req, resp)
}

func cloneConfigured(m *cloneModel) bool {
	if m == nil {
		return false
	}
	return strings.TrimSpace(m.TemplateID.ValueString()) != "" || strings.TrimSpace(m.ID.ValueString()) != "" || strings.TrimSpace(m.Name.ValueString()) != ""
}

func cloudInitConfigured(m *cloudInitModel) bool {
	if m == nil {
		return false
	}
	if m.Hostname.ValueString() != "" || m.User.ValueString() != "" || m.Password.ValueString() != "" || m.Userdata.ValueString() != "" {
		return true
	}
	return !m.SSHKeys.IsNull() && !m.SSHKeys.IsUnknown()
}

func cloudInitFromModel(m *cloudInitModel) (*client.CloudInit, error) {
	if m == nil {
		return nil, nil
	}
	ci := &client.CloudInit{
		Hostname: m.Hostname.ValueString(),
		User:     m.User.ValueString(),
		Password: m.Password.ValueString(),
		Userdata: m.Userdata.ValueString(),
	}
	if !m.SSHKeys.IsNull() && !m.SSHKeys.IsUnknown() {
		var keys []string
		if diags := m.SSHKeys.ElementsAs(context.Background(), &keys, false); diags.HasError() {
			return nil, fmt.Errorf("invalid cloud_init.ssh_keys")
		}
		ci.SSHKeys = keys
	}
	return ci, nil
}

func vmToModel(vm *client.VM, prev vmModel) vmModel {
	console := vm.Spec.ConsoleType
	if console == "" {
		console = "serial"
	}
	m := vmModel{
		ID:             types.StringValue(vm.ID.String()),
		VmID:           types.StringValue(vm.ID.String()),
		Name:           types.StringValue(vm.Spec.Name),
		VCPUs:          types.Int64Value(int64(vm.Spec.VCPUs)),
		MemoryMiB:      types.Int64Value(int64(vm.Spec.MemoryMiB)),
		HA:             types.BoolValue(vm.Spec.HA),
		Autostart:      types.BoolValue(vm.Spec.Autostart),
		AutostartDelay: types.Int64Value(int64(vm.Spec.AutostartDelay)),
		AutostartOrder: types.Int64Value(int64(vm.Spec.AutostartOrder)),
		ConsoleType:    types.StringValue(console),
		Started:        types.BoolValue(vm.State == "running"),
		State:          types.StringValue(vm.State),
		Clone:          prev.Clone,
		CloudInit:      prev.CloudInit,
		ISO:            prev.ISO,
	}
	if vm.NodeID != "" {
		m.NodeID = types.StringValue(vm.NodeID)
	} else {
		m.NodeID = types.StringNull()
	}
	if iso := firstInstallerISO(vm); iso != "" {
		m.ISO = types.StringValue(iso)
	} else if prev.ISO.IsNull() {
		m.ISO = types.StringNull()
	}
	m.Disks = disksToModel(vm, prev.Disks)
	m.Nics = nicsToModel(vm, prev.Nics)
	return m
}

func firstInstallerISO(vm *client.VM) string {
	for _, d := range vm.Spec.Disks {
		if !d.Cdrom || d.ISOName == "" {
			continue
		}
		if strings.Contains(strings.ToLower(d.ISOName), "cidata") {
			continue
		}
		return d.ISOName
	}
	return ""
}

func disksToModel(vm *client.VM, prev []diskModel) []diskModel {
	var out []diskModel
	i := 0
	for _, d := range vm.Spec.Disks {
		if d.Cdrom || d.VolumeID == "" {
			continue
		}
		item := diskModel{
			VolumeID: types.StringValue(d.VolumeID),
			Format:   types.StringValue("qcow2"),
		}
		if i < len(prev) {
			item.Size = prev[i].Size
			item.Name = prev[i].Name
			if !prev[i].Format.IsNull() && prev[i].Format.ValueString() != "" {
				item.Format = prev[i].Format
			}
		} else {
			item.Size = types.StringNull()
			item.Name = types.StringNull()
		}
		out = append(out, item)
		i++
	}
	if len(out) == 0 {
		return prev
	}
	return out
}

func nicsToModel(vm *client.VM, prev []nicModel) []nicModel {
	var out []nicModel
	for i, n := range vm.Spec.Nets {
		item := nicModel{
			NetworkID: types.StringValue(n.NetworkID),
		}
		if n.IP != "" {
			item.IP = types.StringValue(n.IP)
		} else if i < len(prev) {
			item.IP = prev[i].IP
		} else {
			item.IP = types.StringNull()
		}
		if n.Tap != "" {
			item.Tap = types.StringValue(n.Tap)
		} else {
			item.Tap = types.StringNull()
		}
		if n.MAC != "" {
			item.MAC = types.StringValue(n.MAC)
		} else {
			item.MAC = types.StringNull()
		}
		out = append(out, item)
	}
	if len(out) == 0 {
		return prev
	}
	return out
}
