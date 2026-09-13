# Terraform provider for Pertisk

Manage guests, templates, networks, and volumes through the Pertisk HTTP API.

```hcl
terraform {
  required_providers {
    pertisk = {
      source = "pertisktech/pertisk-vms"
    }
  }
}

provider "pertisk" {
  endpoint = "https://node:7480"
  username = "admin"
  password = var.pertisk_password
  insecure = true # self-signed appliance certs
}

resource "pertisk_vms_template" "ubuntu" {
  name  = "ubuntu-24.04"
  image = "./ubuntu-24.04-server-cloudimg-amd64.img"
}

resource "pertisk_vms_vm" "web" {
  name       = "web-1"
  vcpus      = 2
  memory_mib = 2048
  started    = true
  autostart  = true

  clone {
    template_id = pertisk_vms_template.ubuntu.id
    linked      = true
  }

  cloud_init {
    user     = "ubuntu"
    ssh_keys = [file("~/.ssh/id_ed25519.pub")]
  }
}
```

Cloud templates have no NIC. Omit `nic` to attach the cluster's default NAT network, or set `nic { network_id = ... }` to choose one. DHCP/SLAAC then assigns IPv4/IPv6.

## Build

The provider is not on the Terraform Registry yet. Build it from this repo:

```bash
cd terraform-provider-pertisk-vms
go build -o terraform-provider-pertisk-vms
```

Point Terraform at the **directory that contains the binary** (not the binary itself) with a [CLI config](https://developer.hashicorp.com/terraform/cli/config/config-file) `dev_overrides` block (`~/.terraformrc` on macOS/Linux):

```hcl
provider_installation {
  dev_overrides {
    "pertisktech/pertisk-vms" = "/absolute/path/to/pertisk-vms/terraform-provider-pertisk-vms"
  }
  direct {}
}
```

`dev_overrides` skips `terraform init` downloads for that source. Credentials can also come from the environment: `PERTISK_URL`, `PERTISK_USERNAME`, `PERTISK_PASSWORD`, `PERTISK_TOKEN`, `PERTISK_TLS_INSECURE`.

## Resources

| Name | API |
|---|---|
| `pertisk_vms_template` | `POST /v1/templates/import`, `POST /v1/templates`, or `POST /v1/vms/{id}/template` |
| `pertisk_vms_network` | `POST /v1/networks` |
| `pertisk_vms_volume` | `POST /v1/volumes` (resize via `/resize`) |
| `pertisk_vms_vm` | define + attach, or `POST /v1/vms/{id}/clone` |

### `pertisk_vms_template`

Upload a cloud disk image (same as Datacenter → Templates → Import image):

```hcl
resource "pertisk_vms_template" "ubuntu" {
  name       = "ubuntu-24.04"
  image      = "./ubuntu-24.04-server-cloudimg-amd64.img"
  format     = "qcow2" # optional; `.img` / `.qcow2` default to qcow2
  vcpus      = 1
  memory_mib = 1024
}
```

Wrap an already-imported volume, or convert a stopped guest:

```hcl
resource "pertisk_vms_template" "from_vol" {
  name      = "ubuntu-24.04"
  volume_id = pertisk_vms_volume.cloud.id
}

resource "pertisk_vms_template" "golden" {
  name         = "web-golden"
  source_vm_id = pertisk_vms_vm.golden.id
}
```

Set exactly one of `image`, `volume_id`, or `source_vm_id`. Changing the image file (SHA-256) replaces the template.

### `pertisk_vms_vm`

Create a blank guest with a disk and installer ISO:

```hcl
resource "pertisk_vms_vm" "install" {
  vm_id      = "120"
  name       = "alpine"
  vcpus      = 1
  memory_mib = 1024
  iso        = "alpine-virt.iso"
  started    = true

  disk { size = "16G" }
}
```

Clone a template into a guest:

```hcl
resource "pertisk_vms_vm" "web" {
  name       = "web-1"
  vcpus      = 2
  memory_mib = 2048
  started    = true
  autostart  = true

  clone {
    template_id = pertisk_vms_template.ubuntu.id
    linked      = true
    disk_size   = "40G"
  }

  nic {
    network_id = pertisk_vms_network.lan.id
  }

  cloud_init {
    user     = "ubuntu"
    ssh_keys = [file("~/.ssh/id_ed25519.pub")]
  }
}
```

`clone.template_id`, `clone.id`, or `clone.name` selects the source. `started` starts or stops the guest on apply. `autostart` starts the guest when the node boots. Name, vCPU, memory, HA, and autostart update in place. Disk, NIC, ISO, and clone changes replace the guest.

Destroy deletes the guest. Exclusive disks (and cidata ISOs) are removed by the API.

## Data sources

- `pertisk_vms_cluster` — quorum, members
- `pertisk_vms_vm` — lookup by `id` or `name`
- `pertisk_vms_network` — lookup by `id` or `name`
- `pertisk_vms_template` — lookup a cloud template by `id` or `name`

## Import

```bash
terraform import pertisk_vms_vm.web 100
terraform import pertisk_vms_template.ubuntu 100
terraform import pertisk_vms_network.lan <uuid>
terraform import pertisk_vms_volume.disk <uuid>
```
