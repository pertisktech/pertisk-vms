import { useMemo, useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { disksOf, isCloudInitIso, isTemplate, netsOf } from '../api'
import { Btn, Icon } from '../components/Icons'

function quote(value) {
  return `"${String(value ?? '').replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
}

function tfIdent(raw, used) {
  let name = String(raw || 'resource')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
  if (!name) name = 'resource'
  if (/^[0-9]/.test(name)) name = `r_${name}`
  let next = name
  let i = 2
  while (used.has(next)) {
    next = `${name}_${i}`
    i += 1
  }
  used.add(next)
  return next
}

function tfSize(bytes) {
  const n = Number(bytes) || 0
  const g = 1024 ** 3
  const m = 1024 ** 2
  const k = 1024
  if (n >= g && n % g === 0) return `${n / g}G`
  if (n >= m && n % m === 0) return `${n / m}M`
  if (n >= k && n % k === 0) return `${n / k}K`
  return String(n)
}

function indent(text, spaces = 2) {
  const pad = ' '.repeat(spaces)
  return text
    .split('\n')
    .map((line) => (line ? pad + line : line))
    .join('\n')
}

function block(type, name, body) {
  return `${type} "${name}" {\n${indent(body)}\n}`
}

function endpointFromWindow() {
  if (typeof window === 'undefined') return 'http://127.0.0.1:7480'
  return window.location.origin
}

export function inventoryHcl(inv, { endpoint, username } = {}) {
  const url = endpoint || endpointFromWindow()
  const user = username || 'admin'
  const networks = inv?.networks || []
  const volumes = inv?.volumes || []
  const allVms = inv?.vms || []
  const vms = allVms.filter((vm) => !isTemplate(vm))
  const templates = allVms.filter(isTemplate)
  const netNames = new Set()
  const volNames = new Set()
  const vmNames = new Set()
  const tplNames = new Set()
  const netById = new Map()
  const volById = new Map()
  const chunks = []

  const providerLines = [
    'terraform {',
    '  required_providers {',
    '    pertisk_vms = {',
    '      source = "pertisktech/pertisk-vms"',
    '    }',
    '  }',
    '}',
    '',
    'provider "pertisk_vms" {',
    `  endpoint = ${quote(url)}`,
    `  username = ${quote(user)}`,
    '  password = var.pertisk_password',
  ]
  if (url.startsWith('https:')) providerLines.push('  insecure = true')
  providerLines.push('}')
  providerLines.push('')
  providerLines.push('variable "pertisk_password" {')
  providerLines.push('  type      = string')
  providerLines.push('  sensitive = true')
  providerLines.push('}')
  chunks.push(providerLines.join('\n'))

  if (templates.length) {
    const parts = ['# Cloud templates — terraform import pertisk_vms_template.<name> <id>']
    for (const tpl of templates) {
      const ident = tfIdent(tpl.spec?.name || tpl.id, tplNames)
      const spec = tpl.spec || {}
      const inner = [
        `vm_id      = ${quote(String(tpl.id))}`,
        `name       = ${quote(spec.name || tpl.id)}`,
        `vcpus      = ${Number(spec.vcpus) || 1}`,
        `memory_mib = ${Number(spec.memory_mib) || 1024}`,
        '# image   = "./disk.qcow2"   # upload a cloud image',
        '# volume_id = pertisk_vms_volume.cloud.id',
      ].join('\n')
      parts.push(block('resource "pertisk_vms_template"', ident, inner))
    }
    chunks.push(parts.join('\n\n'))
  }

  if (networks.length) {
    const parts = ['# Networks']
    for (const net of networks) {
      const ident = tfIdent(net.name || net.id, netNames)
      netById.set(String(net.id), ident)
      const inner = [
        `name    = ${quote(net.name)}`,
        net.mode ? `mode    = ${quote(net.mode)}` : null,
        net.cidr ? `cidr    = ${quote(net.cidr)}` : null,
        net.gateway ? `gateway = ${quote(net.gateway)}` : null,
        net.mode === 'bridge' && net.bridge ? `bridge  = ${quote(net.bridge)}` : null,
        `dhcp    = ${net.dhcp !== false}`,
        `isolate = ${net.isolate !== false}`,
      ]
        .filter(Boolean)
        .join('\n')
      parts.push(block('resource "pertisk_vms_network"', ident, inner))
    }
    chunks.push(parts.join('\n\n'))
  }

  if (volumes.length) {
    const parts = ['# Volumes']
    for (const vol of volumes) {
      const ident = tfIdent(vol.name || vol.id, volNames)
      volById.set(String(vol.id), ident)
      const inner = [
        `name   = ${quote(vol.name)}`,
        `size   = ${quote(tfSize(vol.size_bytes))}`,
        vol.format ? `format = ${quote(vol.format)}` : null,
      ]
        .filter(Boolean)
        .join('\n')
      parts.push(block('resource "pertisk_vms_volume"', ident, inner))
    }
    chunks.push(parts.join('\n\n'))
  }

  if (vms.length) {
    const parts = ['# Guests']
    for (const vm of vms) {
      const ident = tfIdent(vm.spec?.name || vm.id, vmNames)
      const spec = vm.spec || {}
      const rows = [
        `vm_id      = ${quote(String(vm.id))}`,
        `name       = ${quote(spec.name || vm.id)}`,
        `vcpus      = ${Number(spec.vcpus) || 1}`,
        `memory_mib = ${Number(spec.memory_mib) || 512}`,
        `started    = ${vm.state === 'running'}`,
        `ha         = ${spec.ha !== false}`,
      ]
      if (spec.autostart) rows.push('autostart  = true')
      if (spec.console_type && spec.console_type !== 'serial') {
        rows.push(`console_type = ${quote(spec.console_type)}`)
      }
      const iso = disksOf(vm).find((d) => d.cdrom && d.iso_name && !isCloudInitIso(d.iso_name))
      if (iso?.iso_name) rows.push(`iso        = ${quote(iso.iso_name)}`)

      for (const d of disksOf(vm).filter((disk) => !disk.cdrom && disk.volume_id)) {
        const volIdent = volById.get(String(d.volume_id))
        const inner = volIdent
          ? `volume_id = pertisk_vms_volume.${volIdent}.id`
          : `volume_id = ${quote(d.volume_id)}`
        rows.push(`disk {\n${indent(inner)}\n}`)
      }
      for (const n of netsOf(vm)) {
        const netIdent = netById.get(String(n.network_id))
        const inner = [
          netIdent
            ? `network_id = pertisk_vms_network.${netIdent}.id`
            : n.network_id
              ? `network_id = ${quote(n.network_id)}`
              : null,
          n.ip ? `ip         = ${quote(n.ip)}` : null,
        ]
          .filter(Boolean)
          .join('\n')
        if (inner) rows.push(`nic {\n${indent(inner)}\n}`)
      }
      parts.push(block('resource "pertisk_vms_vm"', ident, rows.join('\n')))
    }
    chunks.push(parts.join('\n\n'))
  }

  if (!networks.length && !vms.length && !volumes.length) {
    chunks.push(`# No guests or networks yet. Example:
#
# resource "pertisk_vms_network" "lan" {
#   name = "lan"
#   mode = "nat"
#   cidr = "10.90.0.0/24"
# }
#
# resource "pertisk_vms_vm" "web" {
#   name       = "web-1"
#   vcpus      = 2
#   memory_mib = 2048
#   started    = true
#   disk { size = "32G" }
#   nic { network_id = pertisk_vms_network.lan.id }
# }`)
  }

  return `${chunks.join('\n\n')}\n`
}

function CodeBlock({ value, label }) {
  const [copied, setCopied] = useState(false)

  async function copy() {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setTimeout(() => setCopied(false), 1600)
    } catch {
      setCopied(false)
    }
  }

  return (
    <div className="tf-code-wrap">
      <Btn icon={copied ? 'check' : 'clone'} variant="secondary" className="tf-copy" onClick={copy}>
        {copied ? 'Copied' : label || 'Copy'}
      </Btn>
      <pre className="tf-code">{value}</pre>
    </div>
  )
}

const SETUP = `provider_installation {
  dev_overrides {
    "pertisktech/pertisk-vms" = "/path/to/pertisk-vms/terraform-provider-pertisk-vms"
  }
  direct {}
}`

const FROM_TEMPLATE = `resource "pertisk_vms_template" "ubuntu" {
  name       = "ubuntu-24.04"
  image      = "./ubuntu-24.04-server-cloudimg-amd64.img"
  vcpus      = 1
  memory_mib = 1024
}

resource "pertisk_vms_vm" "web" {
  name       = "web-1"
  vcpus      = 2
  memory_mib = 2048
  started    = true

  clone {
    template_id = pertisk_vms_template.ubuntu.id
    linked      = true
  }

  cloud_init {
    user     = "ubuntu"
    ssh_keys = [file("~/.ssh/id_ed25519.pub")]
  }
}`

export default function Terraform() {
  const { user, inv } = useOutletContext()
  const endpoint = endpointFromWindow()
  const hcl = useMemo(
    () => inventoryHcl(inv, { endpoint, username: user?.username }),
    [inv, endpoint, user?.username],
  )
  const guests = (inv?.vms || []).filter((vm) => !isTemplate(vm)).length
  const templates = (inv?.vms || []).filter(isTemplate).length

  function download() {
    const blob = new Blob([hcl], { type: 'text/plain' })
    const a = document.createElement('a')
    a.href = URL.createObjectURL(blob)
    a.download = 'main.tf'
    a.click()
    URL.revokeObjectURL(a.href)
  }

  return (
    <div className="dash-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="terraform" size={20} />
            Terraform
          </h1>
          <p className="dash-lead muted">
            Declare guests, networks, and volumes as code. Build the in-repo provider, then apply
            the generated configuration.
          </p>
        </div>
        <div className="dash-resources-actions">
          <Btn icon="clone" variant="secondary" onClick={() => navigator.clipboard.writeText(hcl)}>
            Copy main.tf
          </Btn>
          <Btn icon="updates" variant="secondary" onClick={download}>
            Download main.tf
          </Btn>
        </div>
      </div>

      <section className="card">
        <h2 className="card-title">
          <Icon name="key" size={18} />
          Provider setup
        </h2>
        <p className="muted tf-help">
          From the repo: <code>cd terraform-provider-pertisk-vms && go build</code>. Put this in{' '}
          <code>~/.terraformrc</code> so Terraform uses that binary (not the registry):
        </p>
        <CodeBlock value={SETUP} label="Copy" />
        <p className="muted tf-help">
          Endpoint for this cluster is <code>{endpoint}</code>. Sign in as{' '}
          <code>{user?.username || 'admin'}</code> with the same password you use here, or set{' '}
          <code>PERTISK_TOKEN</code> from <code>pertisk login</code>.
        </p>
      </section>

      <section className="card">
        <h2 className="card-title">
          <Icon name="template" size={18} />
          Upload a template, then clone guests
        </h2>
        <p className="muted tf-help">
          <code>pertisk_vms_template</code> uploads a cloud image (or wraps a volume).{' '}
          <code>pertisk_vms_vm</code> with <code>clone.template_id</code> creates a guest from it.
        </p>
        <CodeBlock value={FROM_TEMPLATE} label="Copy" />
      </section>

      <section className="card">
        <h2 className="card-title">
          <Icon name="template" size={18} />
          Generated configuration
        </h2>
        <p className="muted tf-help">
          Snapshot of current inventory as HCL. Import existing objects with{' '}
          <code>terraform import pertisk_vms_vm.&lt;name&gt; &lt;id&gt;</code> before applying, or use
          this as a starting point for new guests.
        </p>
        <CodeBlock value={hcl} label="Copy HCL" />
      </section>

      <section className="card table-card">
        <h2 className="card-title">
          <Icon name="activity" size={18} />
          Resources
        </h2>
        <div className="table-shell">
          <table>
            <thead>
              <tr>
                <th>Terraform</th>
                <th>Manages</th>
                <th>Here now</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <code>pertisk_vms_network</code>
                </td>
                <td>NAT or bridge networks</td>
                <td>{inv?.networks?.length || 0}</td>
              </tr>
              <tr>
                <td>
                  <code>pertisk_vms_volume</code>
                </td>
                <td>raw / qcow2 disks</td>
                <td>{inv?.volumes?.length || 0}</td>
              </tr>
              <tr>
                <td>
                  <code>pertisk_vms_template</code>
                </td>
                <td>Upload a cloud image, wrap a volume, or convert a guest</td>
                <td>
                  {templates} template{templates === 1 ? '' : 's'}
                </td>
              </tr>
              <tr>
                <td>
                  <code>pertisk_vms_vm</code>
                </td>
                <td>
                  Guests — clone with <code>clone.template_id</code>
                </td>
                <td>
                  {guests} guest{guests === 1 ? '' : 's'}
                </td>
              </tr>
              <tr>
                <td>
                  <code>data.pertisk_vms_cluster</code>
                </td>
                <td>Quorum and members</td>
                <td>{inv?.cluster?.name || '—'}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>
    </div>
  )
}
