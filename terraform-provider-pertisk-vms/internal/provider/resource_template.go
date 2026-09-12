package provider

import (
	"context"

	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/int64default"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringdefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"
	"github.com/hashicorp/terraform-plugin-log/tflog"

	"github.com/pertisktech/pertisk-vms/terraform-provider-pertisk-vms/internal/client"
)

var (
	_ resource.Resource                = &templateResource{}
	_ resource.ResourceWithConfigure   = &templateResource{}
	_ resource.ResourceWithImportState = &templateResource{}
	_ resource.ResourceWithModifyPlan  = &templateResource{}
)

func NewTemplateResource() resource.Resource { return &templateResource{} }

type templateResource struct {
	api *client.Client
}

type templateModel struct {
	ID          types.String `tfsdk:"id"`
	VmID        types.String `tfsdk:"vm_id"`
	Name        types.String `tfsdk:"name"`
	Image       types.String `tfsdk:"image"`
	ImageSHA256 types.String `tfsdk:"image_sha256"`
	VolumeID    types.String `tfsdk:"volume_id"`
	SourceVMID  types.String `tfsdk:"source_vm_id"`
	Format      types.String `tfsdk:"format"`
	VCPUs       types.Int64  `tfsdk:"vcpus"`
	MemoryMiB   types.Int64  `tfsdk:"memory_mib"`
	ConsoleType types.String `tfsdk:"console_type"`
	State       types.String `tfsdk:"state"`
}

func (r *templateResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_template"
}

func (r *templateResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "A cloud template. Upload a disk image, wrap an existing volume, or convert a stopped guest. Clone it with `pertisk_vms_vm` `clone.template_id`.",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				Computed: true,
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"vm_id": schema.StringAttribute{
				Optional:            true,
				Computed:            true,
				MarkdownDescription: "Numeric template ID. Only sent when wrapping a `volume_id`.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplaceIfConfigured(),
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"name": schema.StringAttribute{
				Required: true,
			},
			"image": schema.StringAttribute{
				Optional:            true,
				MarkdownDescription: "Local cloud disk image to upload (qcow2/raw). Mutually exclusive with `volume_id` and `source_vm_id`.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"image_sha256": schema.StringAttribute{
				Computed:            true,
				MarkdownDescription: "SHA-256 of `image`. Changing the file contents replaces the template.",
			},
			"volume_id": schema.StringAttribute{
				Optional:            true,
				MarkdownDescription: "Existing volume to wrap as a template. Mutually exclusive with `image` and `source_vm_id`.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"source_vm_id": schema.StringAttribute{
				Optional:            true,
				MarkdownDescription: "Stopped guest to convert into a template. Mutually exclusive with `image` and `volume_id`.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"format": schema.StringAttribute{
				Optional:            true,
				Computed:            true,
				MarkdownDescription: "`qcow2` or `raw`. Inferred from `image` when omitted (`img`/`qcow2` → qcow2).",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplaceIfConfigured(),
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"vcpus": schema.Int64Attribute{
				Optional: true,
				Computed: true,
				Default:  int64default.StaticInt64(1),
			},
			"memory_mib": schema.Int64Attribute{
				Optional: true,
				Computed: true,
				Default:  int64default.StaticInt64(1024),
			},
			"console_type": schema.StringAttribute{
				Optional: true,
				Computed: true,
				Default:  stringdefault.StaticString("serial"),
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.RequiresReplace(),
				},
			},
			"state": schema.StringAttribute{
				Computed: true,
			},
		},
	}
}

func (r *templateResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	r.api = configureClient(req.ProviderData, &resp.Diagnostics)
}

func (r *templateResource) ModifyPlan(ctx context.Context, req resource.ModifyPlanRequest, resp *resource.ModifyPlanResponse) {
	if req.Plan.Raw.IsNull() {
		return
	}
	var plan templateModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if plan.Image.IsNull() || plan.Image.IsUnknown() || plan.Image.ValueString() == "" {
		return
	}
	pathName := client.ExpandPath(plan.Image.ValueString())
	sum, err := client.FileSHA256(pathName)
	if err != nil {
		resp.Diagnostics.AddAttributeError(path.Root("image"), "Cannot read image", err.Error())
		return
	}
	plan.ImageSHA256 = types.StringValue(sum)
	if plan.Format.IsNull() || plan.Format.IsUnknown() || plan.Format.ValueString() == "" {
		plan.Format = types.StringValue(client.InferImageFormat(pathName))
	}
	if !req.State.Raw.IsNull() {
		var state templateModel
		resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
		if !resp.Diagnostics.HasError() && !state.ImageSHA256.IsNull() && state.ImageSHA256.ValueString() != "" && state.ImageSHA256.ValueString() != sum {
			resp.RequiresReplace = append(resp.RequiresReplace, path.Root("image"))
		}
	}
	resp.Diagnostics.Append(resp.Plan.Set(ctx, &plan)...)
}

func (r *templateResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan templateModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	image := plan.Image.ValueString()
	volumeID := plan.VolumeID.ValueString()
	sourceID := plan.SourceVMID.ValueString()
	n := 0
	if image != "" {
		n++
	}
	if volumeID != "" {
		n++
	}
	if sourceID != "" {
		n++
	}
	if n != 1 {
		resp.Diagnostics.AddError(
			"Invalid template source",
			"Set exactly one of image, volume_id, or source_vm_id.",
		)
		return
	}

	var vm *client.VM
	var err error
	vcpus := int(plan.VCPUs.ValueInt64())
	mem := int(plan.MemoryMiB.ValueInt64())
	switch {
	case image != "":
		format := plan.Format.ValueString()
		if format == "" {
			format = client.InferImageFormat(image)
		}
		tflog.Info(ctx, "uploading template image", map[string]any{"name": plan.Name.ValueString(), "image": image})
		vm, err = r.api.ImportTemplate(plan.Name.ValueString(), format, image, vcpus, mem)
	case volumeID != "":
		body := client.CreateTemplateRequest{
			Name:        plan.Name.ValueString(),
			VolumeID:    volumeID,
			VCPUs:       &vcpus,
			MemoryMiB:   &mem,
			ConsoleType: plan.ConsoleType.ValueString(),
		}
		if !plan.VmID.IsNull() && !plan.VmID.IsUnknown() && plan.VmID.ValueString() != "" {
			body.ID = client.NumericID(plan.VmID.ValueString())
		}
		tflog.Info(ctx, "creating template from volume", map[string]any{"name": body.Name, "volume": volumeID})
		vm, err = r.api.CreateTemplate(body)
	default:
		tflog.Info(ctx, "converting guest to template", map[string]any{"vm": sourceID})
		vm, err = r.api.ConvertToTemplate(sourceID)
		if err == nil && vm != nil {
			patch := client.UpdateVMRequest{}
			if plan.Name.ValueString() != "" && plan.Name.ValueString() != vm.Spec.Name {
				name := plan.Name.ValueString()
				patch.Name = &name
			}
			if vcpus > 0 && vcpus != vm.Spec.VCPUs {
				patch.VCPUs = intPtr(vcpus)
			}
			if mem > 0 && mem != vm.Spec.MemoryMiB {
				patch.MemoryMiB = intPtr(mem)
			}
			if patch.Name != nil || patch.VCPUs != nil || patch.MemoryMiB != nil {
				vm, err = r.api.UpdateVM(vm.ID.String(), patch)
			}
		}
	}
	if err != nil {
		resp.Diagnostics.AddError("Create template failed", err.Error())
		return
	}
	state := templateToModel(vm, plan)
	resp.Diagnostics.Append(resp.State.Set(ctx, &state)...)
}

func (r *templateResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state templateModel
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
		resp.Diagnostics.AddError("Read template failed", err.Error())
		return
	}
	next := templateToModel(vm, state)
	if image := state.Image.ValueString(); image != "" {
		if sum, err := client.FileSHA256(client.ExpandPath(image)); err == nil {
			next.ImageSHA256 = types.StringValue(sum)
		}
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, &next)...)
}

func (r *templateResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan, state templateModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
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
	vm, err := r.api.UpdateVM(state.ID.ValueString(), patch)
	if err != nil {
		resp.Diagnostics.AddError("Update template failed", err.Error())
		return
	}
	next := templateToModel(vm, plan)
	resp.Diagnostics.Append(resp.State.Set(ctx, &next)...)
}

func (r *templateResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state templateModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if err := r.api.DeleteVM(state.ID.ValueString()); err != nil && !client.IsNotFound(err) {
		resp.Diagnostics.AddError("Delete template failed", err.Error())
	}
}

func (r *templateResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("id"), req, resp)
}

func templateToModel(vm *client.VM, prev templateModel) templateModel {
	console := vm.Spec.ConsoleType
	if console == "" {
		console = "serial"
	}
	m := templateModel{
		ID:          types.StringValue(vm.ID.String()),
		VmID:        types.StringValue(vm.ID.String()),
		Name:        types.StringValue(vm.Spec.Name),
		VCPUs:       types.Int64Value(int64(vm.Spec.VCPUs)),
		MemoryMiB:   types.Int64Value(int64(vm.Spec.MemoryMiB)),
		ConsoleType: types.StringValue(console),
		State:       types.StringValue(vm.State),
		Image:       prev.Image,
		ImageSHA256: prev.ImageSHA256,
		VolumeID:    prev.VolumeID,
		SourceVMID:  prev.SourceVMID,
		Format:      prev.Format,
	}
	if m.Format.IsNull() || m.Format.ValueString() == "" {
		if prev.Image.ValueString() != "" {
			m.Format = types.StringValue(client.InferImageFormat(prev.Image.ValueString()))
		} else {
			m.Format = types.StringNull()
		}
	}
	return m
}
