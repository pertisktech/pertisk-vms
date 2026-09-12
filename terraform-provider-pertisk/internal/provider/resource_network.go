package provider

import (
	"context"

	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/booldefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/boolplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringdefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"

	"github.com/pertisktech/pertisk-vms/terraform-provider-pertisk/internal/client"
)

var (
	_ resource.Resource                = &networkResource{}
	_ resource.ResourceWithConfigure   = &networkResource{}
	_ resource.ResourceWithImportState = &networkResource{}
)

func NewNetworkResource() resource.Resource { return &networkResource{} }

type networkResource struct {
	api *client.Client
}

type networkModel struct {
	ID      types.String `tfsdk:"id"`
	Name    types.String `tfsdk:"name"`
	Mode    types.String `tfsdk:"mode"`
	CIDR    types.String `tfsdk:"cidr"`
	Gateway types.String `tfsdk:"gateway"`
	Bridge  types.String `tfsdk:"bridge"`
	DHCP    types.Bool   `tfsdk:"dhcp"`
	Isolate types.Bool   `tfsdk:"isolate"`
}

func (r *networkResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_network"
}

func (r *networkResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "A virtual network (NAT pool or host bridge).",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				Computed: true,
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"name": schema.StringAttribute{
				Required:            true,
				MarkdownDescription: "Network name.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"mode": schema.StringAttribute{
				Optional:            true,
				Computed:            true,
				Default:             stringdefault.StaticString("nat"),
				MarkdownDescription: "`nat` or `bridge`.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"cidr": schema.StringAttribute{
				Optional:            true,
				Computed:            true,
				MarkdownDescription: "IPv4 pool, e.g. `10.90.0.0/24`. Ignored for typical bridge mode.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"gateway": schema.StringAttribute{
				Optional: true,
				Computed: true,
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"bridge": schema.StringAttribute{
				Optional:            true,
				Computed:            true,
				MarkdownDescription: "Existing host bridge for `mode = bridge` (e.g. `br0`).",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"dhcp": schema.BoolAttribute{
				Optional: true,
				Computed: true,
				Default:  booldefault.StaticBool(true),
				PlanModifiers: []planmodifier.Bool{
					boolplanmodifier.RequiresReplace(),
				},
			},
			"isolate": schema.BoolAttribute{
				Optional: true,
				Computed: true,
				Default:  booldefault.StaticBool(true),
				PlanModifiers: []planmodifier.Bool{
					boolplanmodifier.RequiresReplace(),
				},
			},
		},
	}
}

func (r *networkResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	r.api = configureClient(req.ProviderData, &resp.Diagnostics)
}

func (r *networkResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan networkModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	dhcp := plan.DHCP.ValueBool()
	isolate := plan.Isolate.ValueBool()
	body := client.CreateNetworkRequest{
		Name:    plan.Name.ValueString(),
		Mode:    plan.Mode.ValueString(),
		CIDR:    plan.CIDR.ValueString(),
		Gateway: plan.Gateway.ValueString(),
		Bridge:  plan.Bridge.ValueString(),
		DHCP:    &dhcp,
		Isolate: &isolate,
	}
	net, err := r.api.CreateNetwork(body)
	if err != nil {
		resp.Diagnostics.AddError("Create network failed", err.Error())
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, networkToModel(net))...)
}

func (r *networkResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state networkModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	net, err := r.api.GetNetwork(state.ID.ValueString())
	if client.IsNotFound(err) {
		resp.State.RemoveResource(ctx)
		return
	}
	if err != nil {
		resp.Diagnostics.AddError("Read network failed", err.Error())
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, networkToModel(net))...)
}

func (r *networkResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	resp.Diagnostics.AddError("Networks cannot be updated in place", "Change requires replace. Terraform should have planned a replace.")
}

func (r *networkResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state networkModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if err := r.api.DeleteNetwork(state.ID.ValueString()); err != nil && !client.IsNotFound(err) {
		resp.Diagnostics.AddError("Delete network failed", err.Error())
	}
}

func (r *networkResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("id"), req, resp)
}

func networkToModel(net *client.Network) networkModel {
	m := networkModel{
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
	if net.Mode == "" {
		m.Mode = types.StringValue("nat")
	}
	return m
}
