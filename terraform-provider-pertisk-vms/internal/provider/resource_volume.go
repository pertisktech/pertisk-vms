package provider

import (
	"context"

	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/int64planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringdefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"

	"github.com/pertisktech/pertisk-vms/terraform-provider-pertisk-vms/internal/client"
)

var (
	_ resource.Resource                = &volumeResource{}
	_ resource.ResourceWithConfigure   = &volumeResource{}
	_ resource.ResourceWithImportState = &volumeResource{}
)

func NewVolumeResource() resource.Resource { return &volumeResource{} }

type volumeResource struct {
	api *client.Client
}

type volumeModel struct {
	ID           types.String `tfsdk:"id"`
	Name         types.String `tfsdk:"name"`
	Size         types.String `tfsdk:"size"`
	SizeBytes    types.Int64  `tfsdk:"size_bytes"`
	Format       types.String `tfsdk:"format"`
	Replicas     types.Int64  `tfsdk:"replicas"`
	ReplicaCount types.Int64  `tfsdk:"replica_count"`
}

func (r *volumeResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_volume"
}

func (r *volumeResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "A disk volume (raw or qcow2). Growing `size` resizes in place; shrinking is rejected by the API.",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				Computed: true,
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"name": schema.StringAttribute{
				Required: true,
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"size": schema.StringAttribute{
				Required:            true,
				MarkdownDescription: "Size such as `32G` or `512M`.",
			},
			"size_bytes": schema.Int64Attribute{
				Computed: true,
			},
			"format": schema.StringAttribute{
				Optional:            true,
				Computed:            true,
				Default:             stringdefault.StaticString("qcow2"),
				MarkdownDescription: "`qcow2` or `raw`.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"replicas": schema.Int64Attribute{
				Optional:            true,
				MarkdownDescription: "Desired replica count. Unset uses the cluster default.",
				PlanModifiers: []planmodifier.Int64{
					int64planmodifier.RequiresReplace(),
				},
			},
			"replica_count": schema.Int64Attribute{
				Computed: true,
			},
		},
	}
}

func (r *volumeResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	r.api = configureClient(req.ProviderData, &resp.Diagnostics)
}

func (r *volumeResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan volumeModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	bytes, err := client.ParseSize(plan.Size.ValueString())
	if err != nil {
		resp.Diagnostics.AddAttributeError(path.Root("size"), "Invalid size", err.Error())
		return
	}
	body := client.CreateVolumeRequest{
		Name:      plan.Name.ValueString(),
		SizeBytes: bytes,
		Format:    plan.Format.ValueString(),
	}
	if !plan.Replicas.IsNull() && !plan.Replicas.IsUnknown() {
		n := int(plan.Replicas.ValueInt64())
		body.Replicas = &n
	}
	vol, err := r.api.CreateVolume(body)
	if err != nil {
		resp.Diagnostics.AddError("Create volume failed", err.Error())
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, volumeToModel(vol, plan))...)
}

func (r *volumeResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state volumeModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	vol, err := r.api.GetVolume(state.ID.ValueString())
	if client.IsNotFound(err) {
		resp.State.RemoveResource(ctx)
		return
	}
	if err != nil {
		resp.Diagnostics.AddError("Read volume failed", err.Error())
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, volumeToModel(vol, state))...)
}

func (r *volumeResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan, state volumeModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	bytes, err := client.ParseSize(plan.Size.ValueString())
	if err != nil {
		resp.Diagnostics.AddAttributeError(path.Root("size"), "Invalid size", err.Error())
		return
	}
	vol, err := r.api.ResizeVolume(state.ID.ValueString(), bytes)
	if err != nil {
		resp.Diagnostics.AddError("Resize volume failed", err.Error())
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, volumeToModel(vol, plan))...)
}

func (r *volumeResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state volumeModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if err := r.api.DeleteVolume(state.ID.ValueString()); err != nil && !client.IsNotFound(err) {
		resp.Diagnostics.AddError("Delete volume failed", err.Error())
	}
}

func (r *volumeResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("id"), req, resp)
}

func volumeToModel(vol *client.Volume, prev volumeModel) volumeModel {
	m := volumeModel{
		ID:           types.StringValue(vol.ID),
		Name:         types.StringValue(vol.Name),
		Format:       types.StringValue(vol.Format),
		SizeBytes:    types.Int64Value(int64(vol.SizeBytes)),
		Size:         types.StringValue(client.FormatSize(vol.SizeBytes)),
		ReplicaCount: types.Int64Value(int64(vol.ReplicaCount)),
		Replicas:     prev.Replicas,
	}
	if !prev.Size.IsNull() && !prev.Size.IsUnknown() {
		if want, err := client.ParseSize(prev.Size.ValueString()); err == nil && want == vol.SizeBytes {
			m.Size = prev.Size
		}
	}
	return m
}
