terraform {
  required_providers {
    pertisk = {
      source = "pertisktech/pertisk-vms"
    }
  }
}

provider "pertisk" {
  endpoint = var.endpoint
  username = var.username
  password = var.password
  insecure = var.insecure
}

variable "endpoint" {
  type    = string
  default = "http://127.0.0.1:7480"
}

variable "username" {
  type    = string
  default = "admin"
}

variable "password" {
  type      = string
  sensitive = true
}

variable "insecure" {
  type    = bool
  default = false
}

data "pertisk_vms_cluster" "this" {}

resource "pertisk_vms_template" "ubuntu" {
  name       = "ubuntu-24.04"
  image      = "./ubuntu-24.04-server-cloudimg-amd64.img"
  vcpus      = 1
  memory_mib = 1024
}

resource "pertisk_vms_network" "lan" {
  name = "tf-lan"
  mode = "nat"
  cidr = "10.94.0.0/24"
}

resource "pertisk_vms_vm" "web" {
  vm_id      = "110"
  name       = "tf-web-1"
  vcpus      = 2
  memory_mib = 2048
  started    = true
  autostart  = true
  ha         = true

  clone {
    template_id = pertisk_vms_template.ubuntu.id
    linked      = true
  }

  nic {
    network_id = pertisk_vms_network.lan.id
  }

  cloud_init {
    hostname = "tf-web-1"
    user     = "ubuntu"
    ssh_keys = [
      file("~/.ssh/id_ed25519.pub"),
    ]
  }
}

output "cluster_quorum" {
  value = data.pertisk_vms_cluster.this.quorum
}

output "web_id" {
  value = pertisk_vms_vm.web.vm_id
}
