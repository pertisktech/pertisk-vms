package provider

import (
	"context"

	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/datasource/schema"
	"github.com/hashicorp/terraform-plugin-framework/types"

	"github.com/pertisktech/pertisk-vms/terraform-provider-pertisk/internal/client"
)

func NewClusterDataSource() datasource.DataSource { return &clusterDataSource{} }
func NewVMDataSource() datasource.DataSource      { return &vmDataSource{} }
func NewNetworkDataSource() datasource.DataSource { return &networkDataSource{} }
func NewTemplateDataSource() datasource.DataSource {
	return &templateDataSource{}
}

type clusterDataSource struct{ api *client.Client }
type vmDataSource struct{ api *client.Client }
type networkDataSource struct{ api *client.Client }
type templateDataSource struct{ api *client.Client }

type clusterMemberModel struct {
	ID        types.String `tfsdk:"id"`
	Name      types.String `tfsdk:"name"`
	Online    types.Bool   `tfsdk:"online"`
	PeerURL   types.String `tfsdk:"peer_url"`
	CPUs      types.Int64  `tfsdk:"cpus"`
	MemoryMiB types.Int64  `tfsdk:"memory_mib"`
}

type clusterDataModel struct {
	ID         types.String         `tfsdk:"id"`
	Name       types.String         `tfsdk:"name"`
	Generation types.Int64          `tfsdk:"generation"`
	SelfID     types.String         `tfsdk:"self_id"`
	LeaderID   types.String         `tfsdk:"leader_id"`
	Quorum     types.Bool           `tfsdk:"quorum"`
	Fenced     types.Bool           `tfsdk:"fenced"`
	Members    []clusterMemberModel `tfsdk:"members"`
}

type vmDataModel struct {
	ID        types.String `tfsdk:"id"`
	Name      types.String `tfsdk:"name"`
	VCPUs     types.Int64  `tfsdk:"vcpus"`
	MemoryMiB types.Int64  `tfsdk:"memory_mib"`
	State     types.String `tfsdk:"state"`
	NodeID    types.String `tfsdk:"node_id"`
	HA        types.Bool   `tfsdk:"ha"`
	Template  types.Bool   `tfsdk:"template"`
}

type networkDataModel struct {
	ID      types.String `tfsdk:"id"`
	Name    types.String `tfsdk:"name"`
	Mode    types.String `tfsdk:"mode"`
	CIDR    types.String `tfsdk:"cidr"`
	Bridge  types.String `tfsdk:"bridge"`
	Gateway types.String `tfsdk:"gateway"`
	DHCP    types.Bool   `tfsdk:"dhcp"`
	Isolate types.Bool   `tfsdk:"isolate"`
}

func (d *clusterDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_cluster"
}
func (d *vmDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_vm"
}
func (d *networkDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_network"
}
func (d *templateDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_template"
}

func (d *clusterDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Current cluster membership and quorum.",
		Attributes: map[string]schema.Attribute{
			"id":         schema.StringAttribute{Computed: true},
			"name":       schema.StringAttribute{Computed: true},
			"generation": schema.Int64Attribute{Computed: true},
			"self_id":    schema.StringAttribute{Computed: true},
			"leader_id":  schema.StringAttribute{Computed: true},
			"quorum":     schema.BoolAttribute{Computed: true},
			"fenced":     schema.BoolAttribute{Computed: true},
			"members": schema.ListNestedAttribute{
				Computed: true,
				NestedObject: schema.NestedAttributeObject{
					Attributes: map[string]schema.Attribute{
						"id":         schema.StringAttribute{Computed: true},
						"name":       schema.StringAttribute{Computed: true},
						"online":     schema.BoolAttribute{Computed: true},
						"peer_url":   schema.StringAttribute{Computed: true},
						"cpus":       schema.Int64Attribute{Computed: true},
						"memory_mib": schema.Int64Attribute{Computed: true},
					},
				},
			},
		},
	}
}

func (d *vmDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Look up a guest by `id` or `name`.",
		Attributes: map[string]schema.Attribute{
			"id":         schema.StringAttribute{Optional: true, Computed: true},
			"name":       schema.StringAttribute{Optional: true, Computed: true},
			"vcpus":      schema.Int64Attribute{Computed: true},
			"memory_mib": schema.Int64Attribute{Computed: true},
			"state":      schema.StringAttribute{Computed: true},
			"node_id":    schema.StringAttribute{Computed: true},
			"ha":         schema.BoolAttribute{Computed: true},
			"template":   schema.BoolAttribute{Computed: true},
		},
	}
}

func (d *networkDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Look up a network by `id` or `name`.",
		Attributes: map[string]schema.Attribute{
			"id":      schema.StringAttribute{Optional: true, Computed: true},
			"name":    schema.StringAttribute{Optional: true, Computed: true},
			"mode":    schema.StringAttribute{Computed: true},
			"cidr":    schema.StringAttribute{Computed: true},
			"bridge":  schema.StringAttribute{Computed: true},
			"gateway": schema.StringAttribute{Computed: true},
			"dhcp":    schema.BoolAttribute{Computed: true},
			"isolate": schema.BoolAttribute{Computed: true},
		},
	}
}

func (d *templateDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Look up a cloud template by `id` or `name`.",
		Attributes: map[string]schema.Attribute{
			"id":         schema.StringAttribute{Optional: true, Computed: true},
			"name":       schema.StringAttribute{Optional: true, Computed: true},
			"vcpus":      schema.Int64Attribute{Computed: true},
			"memory_mib": schema.Int64Attribute{Computed: true},
			"state":      schema.StringAttribute{Computed: true},
			"node_id":    schema.StringAttribute{Computed: true},
			"ha":         schema.BoolAttribute{Computed: true},
			"template":   schema.BoolAttribute{Computed: true},
		},
	}
}

func (d *clusterDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	d.api = configureClient(req.ProviderData, &resp.Diagnostics)
}
func (d *vmDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	d.api = configureClient(req.ProviderData, &resp.Diagnostics)
}
func (d *networkDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	d.api = configureClient(req.ProviderData, &resp.Diagnostics)
}
func (d *templateDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	d.api = configureClient(req.ProviderData, &resp.Diagnostics)
}

func (d *clusterDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	cl, err := d.api.Cluster()
	if err != nil {
		resp.Diagnostics.AddError("Read cluster failed", err.Error())
		return
	}
	m := clusterDataModel{
		ID:         types.StringValue(cl.SelfID),
		Name:       types.StringValue(cl.Name),
		Generation: types.Int64Value(int64(cl.Generation)),
		SelfID:     types.StringValue(cl.SelfID),
		LeaderID:   types.StringNull(),
		Quorum:     types.BoolValue(cl.Quorum),
		Fenced:     types.BoolValue(cl.Fenced),
		Members:    []clusterMemberModel{},
	}
	if cl.LeaderID != "" {
		m.LeaderID = types.StringValue(cl.LeaderID)
	} else {
		m.LeaderID = types.StringNull()
	}
	for _, mem := range cl.Members {
		m.Members = append(m.Members, clusterMemberModel{
			ID:        types.StringValue(mem.ID),
			Name:      types.StringValue(mem.Name),
			Online:    types.BoolValue(mem.Online),
			PeerURL:   types.StringValue(mem.PeerURL),
			CPUs:      types.Int64Value(int64(mem.CPUs)),
			MemoryMiB: types.Int64Value(int64(mem.MemoryMiB)),
		})
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, &m)...)
}

func (d *vmDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	var cfg vmDataModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &cfg)...)
	if resp.Diagnostics.HasError() {
		return
	}
	key := cfg.ID.ValueString()
	if key == "" {
		key = cfg.Name.ValueString()
	}
	if key == "" {
		resp.Diagnostics.AddError("Missing lookup", "Set id or name.")
		return
	}
	vm, err := d.api.FindVM(key)
	if err != nil {
		resp.Diagnostics.AddError("Read guest failed", err.Error())
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, vmToData(vm))...)
}

func (d *networkDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	var cfg networkDataModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &cfg)...)
	if resp.Diagnostics.HasError() {
		return
	}
	key := cfg.ID.ValueString()
	if key == "" {
		key = cfg.Name.ValueString()
	}
	if key == "" {
		resp.Diagnostics.AddError("Missing lookup", "Set id or name.")
		return
	}
	net, err := d.api.FindNetwork(key)
	if err != nil {
		resp.Diagnostics.AddError("Read network failed", err.Error())
		return
	}
	m := networkDataModel{
		ID:      types.StringValue(net.ID),
		Name:    types.StringValue(net.Name),
		Mode:    types.StringValue(net.Mode),
		CIDR:    types.StringValue(net.CIDR),
		Bridge:  types.StringValue(net.Bridge),
		DHCP:    types.BoolValue(net.DHCP),
		Isolate: types.BoolValue(net.Isolate),
	}
	if net.Gateway != "" {
		m.Gateway = types.StringValue(net.Gateway)
	} else {
		m.Gateway = types.StringNull()
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, &m)...)
}

func (d *templateDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	var cfg vmDataModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &cfg)...)
	if resp.Diagnostics.HasError() {
		return
	}
	key := cfg.ID.ValueString()
	if key == "" {
		key = cfg.Name.ValueString()
	}
	if key == "" {
		resp.Diagnostics.AddError("Missing lookup", "Set id or name.")
		return
	}
	vm, err := d.api.FindTemplate(key)
	if err != nil {
		resp.Diagnostics.AddError("Read template failed", err.Error())
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, vmToData(vm))...)
}

func vmToData(vm *client.VM) vmDataModel {
	m := vmDataModel{
		ID:        types.StringValue(vm.ID.String()),
		Name:      types.StringValue(vm.Spec.Name),
		VCPUs:     types.Int64Value(int64(vm.Spec.VCPUs)),
		MemoryMiB: types.Int64Value(int64(vm.Spec.MemoryMiB)),
		State:     types.StringValue(vm.State),
		HA:        types.BoolValue(vm.Spec.HA),
		Template:  types.BoolValue(vm.Template),
	}
	if vm.NodeID != "" {
		m.NodeID = types.StringValue(vm.NodeID)
	} else {
		m.NodeID = types.StringNull()
	}
	return m
}
