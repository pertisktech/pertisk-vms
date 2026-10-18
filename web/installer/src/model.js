export const VERSION = '0.1.0'

export const STEPS = [
  { id: 'welcome', title: 'Welcome', hint: 'Locale and this machine' },
  { id: 'disk', title: 'Disk', hint: 'Where the node is installed' },
  { id: 'network', title: 'Network', hint: 'Address and hostname' },
  { id: 'access', title: 'Access', hint: 'Administrator password' },
  { id: 'review', title: 'Review', hint: 'Confirm and install' },
]

export const LANGUAGES = [
  { id: 'en_US.UTF-8', label: 'English (United States)' },
  { id: 'en_GB.UTF-8', label: 'English (United Kingdom)' },
  { id: 'de_DE.UTF-8', label: 'Deutsch' },
  { id: 'fr_FR.UTF-8', label: 'Français' },
  { id: 'es_ES.UTF-8', label: 'Español' },
  { id: 'ja_JP.UTF-8', label: '日本語' },
]

export const KEYBOARDS = [
  { id: 'us', label: 'US' },
  { id: 'gb', label: 'UK' },
  { id: 'de', label: 'German' },
  { id: 'fr', label: 'French' },
  { id: 'jp', label: 'Japanese' },
]

export const TIMEZONES = [
  'Etc/UTC',
  'America/New_York',
  'America/Chicago',
  'America/Denver',
  'America/Los_Angeles',
  'Europe/London',
  'Europe/Berlin',
  'Asia/Bangkok',
  'Asia/Tokyo',
  'Australia/Sydney',
]

export const HOST = {
  cpu: 'AMD EPYC 9354P',
  threads: 32,
  memory: '64 GiB',
  firmware: 'UEFI',
  secureBoot: false,
  virt: 'SVM',
}

export const DISKS = [
  {
    id: 'nvme0n1',
    model: 'Samsung SSD 990 PRO 2TB',
    size: 2000398934016,
    bus: 'NVMe',
    kind: 'disk',
  },
  {
    id: 'sda',
    model: 'WDC WDS500G2B0A',
    size: 500107862016,
    bus: 'SATA',
    kind: 'disk',
  },
  {
    id: 'sdb',
    model: 'SanDisk Ultra Fit',
    size: 30943995904,
    bus: 'USB',
    kind: 'installer',
  },
]

export const NICS = [
  {
    id: 'eno1',
    model: 'Intel I226-V',
    mac: 'a4:bb:6d:12:8c:01',
    link: true,
    speed: '2.5 Gb/s',
    dhcp: '10.1.1.42',
  },
  {
    id: 'eno2',
    model: 'Intel I226-V',
    mac: 'a4:bb:6d:12:8c:02',
    link: false,
    speed: null,
    dhcp: null,
  },
]

const GiB = 1024 ** 3
const MiB = 1024 ** 2
const TiB = 1024 ** 4

export function formatBytes(n) {
  if (n >= TiB) return `${(n / TiB).toFixed(2)} TiB`
  if (n >= GiB) {
    const whole = n / GiB
    return Number.isInteger(whole) || n >= 10 * GiB
      ? `${Math.round(whole)} GiB`
      : `${whole.toFixed(1)} GiB`
  }
  return `${Math.round(n / MiB)} MiB`
}

export function partitionPlan(bytes) {
  const efi = 512 * MiB
  const boot = GiB
  const swap = 2 * GiB
  const rootCap = 70 * GiB
  const rest = Math.max(0, bytes - efi - boot - swap)
  const root = Math.min(rootCap, rest)
  const guests = Math.max(0, rest - root)
  return [
    { id: 'efi', label: 'EFI', mount: '/boot/efi', bytes: efi },
    { id: 'boot', label: 'Boot', mount: '/boot', bytes: boot },
    { id: 'swap', label: 'Swap', mount: 'swap', bytes: swap },
    { id: 'root', label: 'System', mount: '/', bytes: root },
    { id: 'guests', label: 'Guests', mount: '/var/lib/pertisk', bytes: guests },
  ]
}

const catalog = {
  live: false,
  version: VERSION,
  host: HOST,
  disks: DISKS,
  nics: NICS,
}

export function isLive() {
  return catalog.live
}

export function productVersion() {
  return catalog.version || VERSION
}

export function hostInfo() {
  return catalog.host
}

export function diskList() {
  return catalog.disks
}

export function nicList() {
  return catalog.nics
}

export function applySession(session) {
  catalog.live = true
  catalog.version = session.version || VERSION
  catalog.host = session.host || HOST
  catalog.disks = Array.isArray(session.disks) ? session.disks : []
  catalog.nics = Array.isArray(session.nics) ? session.nics : []
}

export function findDisk(id) {
  return catalog.disks.find((d) => d.id === id) || null
}

export function findNic(id) {
  return catalog.nics.find((n) => n.id === id) || null
}

export function managementAddress(form) {
  if (form.mode === 'static') return form.address.trim()
  const nic = findNic(form.nicId)
  return nic?.link && nic.dhcp ? nic.dhcp : ''
}

export function uiUrls(form) {
  const ip = managementAddress(form)
  if (!ip) return { https: '', http: '' }
  return { https: `https://${ip}:7443/`, http: `http://${ip}:7480/` }
}

export function languageLabel(id) {
  return LANGUAGES.find((l) => l.id === id)?.label || id
}

export function keyboardLabel(id) {
  return KEYBOARDS.find((k) => k.id === id)?.label || id
}

export function initialForm() {
  return {
    lang: 'en_US.UTF-8',
    keyboard: 'us',
    timezone: 'Etc/UTC',
    diskId: 'nvme0n1',
    eraseConfirmed: false,
    nicId: 'eno1',
    mode: 'dhcp',
    address: '10.1.1.42',
    prefix: '24',
    gateway: '10.1.1.1',
    dns: '1.1.1.1',
    hostname: 'pertisk-node',
    peer: '',
    adminPassword: '',
    adminConfirm: '',
    separateRoot: false,
    rootPassword: '',
    rootConfirm: '',
  }
}

function ipv4(value) {
  const match = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(value.trim())
  if (!match) return false
  return match.slice(1).every((part) => Number(part) <= 255)
}

function passwordIssue(pw) {
  if (!pw) return 'Password is required.'
  if (pw.length < 8) return 'Use at least 8 characters.'
  if (pw.includes(':') || pw.includes('\n')) return 'Password cannot contain a colon.'
  return ''
}

export function passwordScore(pw) {
  if (!pw || pw.length < 8) return 0
  let score = 1
  if (pw.length >= 12) score += 1
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score += 1
  if (/\d/.test(pw) && /[^A-Za-z0-9]/.test(pw)) score += 1
  return score
}

export function passwordLabel(pw) {
  if (!pw) return ''
  if (pw.length < 8) return 'At least 8 characters'
  return ['', 'Weak', 'Okay', 'Good', 'Strong'][passwordScore(pw)]
}

export function issues(stepId, form) {
  const fields = {}
  let formError = ''

  if (stepId === 'disk') {
    const disk = findDisk(form.diskId)
    if (!disk || disk.kind !== 'disk') formError = 'Select an internal disk.'
    else if (disk.size < 24 * GiB) formError = 'Choose a disk of at least 24 GiB.'
    else if (!form.eraseConfirmed) fields.erase = 'Confirm this disk can be erased.'
  }

  if (stepId === 'network') {
    const host = form.hostname.trim()
    if (!host) fields.hostname = 'Hostname is required.'
    else if (!/^[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(host)) {
      fields.hostname = 'Use a short name: letters, digits, and hyphens.'
    }
    if (!findNic(form.nicId)) formError = 'Select a network interface.'
    if (form.mode === 'static') {
      if (!ipv4(form.address)) fields.address = 'Enter an IPv4 address.'
      const prefix = Number(form.prefix)
      if (!Number.isInteger(prefix) || prefix < 1 || prefix > 32) fields.prefix = 'Prefix must be 1–32.'
      if (!ipv4(form.gateway)) fields.gateway = 'Enter a gateway.'
      if (!ipv4(form.dns)) fields.dns = 'Enter a DNS server.'
    }
    const peer = form.peer.trim()
    if (peer && !/^https?:\/\/\S+$/.test(peer)) fields.peer = 'Use an http:// or https:// URL.'
  }

  if (stepId === 'access') {
    const admin = passwordIssue(form.adminPassword)
    if (admin) fields.adminPassword = admin
    else if (form.adminConfirm !== form.adminPassword) fields.adminConfirm = 'Passwords do not match.'
    if (form.separateRoot) {
      const root = passwordIssue(form.rootPassword)
      if (root) fields.rootPassword = root
      else if (form.rootConfirm !== form.rootPassword) fields.rootConfirm = 'Passwords do not match.'
    }
  }

  return { formError, fields }
}

export function hasIssues(issue) {
  return Boolean(issue.formError || Object.keys(issue.fields).length)
}

export function firstInvalidStep(form) {
  for (let i = 0; i < STEPS.length - 1; i += 1) {
    if (hasIssues(issues(STEPS[i].id, form))) return i
  }
  return -1
}

export function installScript(form) {
  const disk = findDisk(form.diskId)
  const nic = findNic(form.nicId)
  const plan = disk ? partitionPlan(disk.size) : []
  const urls = uiUrls(form)
  return [
    {
      id: 'part',
      label: 'Partitioning disk',
      lines: [
        `Creating a GPT label on /dev/${disk?.id || 'disk'}`,
        ...plan.map((part) => `${part.mount}  ${formatBytes(part.bytes)}`),
      ],
    },
    {
      id: 'base',
      label: 'Installing AlmaLinux 10',
      lines: [
        'BaseOS and AppStream from repo.almalinux.org',
        'minimal-environment, qemu-kvm, qemu-img',
        'NetworkManager, OpenSSH, firewalld',
      ],
    },
    {
      id: 'pkg',
      label: 'Installing Pertisk',
      lines: [
        'pertisk-vms from the install media',
        'pertiskd and Cloud Hypervisor',
      ],
    },
    {
      id: 'cfg',
      label: 'Configuring the node',
      lines: [
        `Hostname ${form.hostname.trim() || 'pertisk-node'}`,
        form.mode === 'dhcp'
          ? `${nic?.id || 'nic'}  DHCP`
          : `${nic?.id || 'nic'}  ${form.address.trim()}/${form.prefix}`,
        form.peer.trim() ? `Cluster peer ${form.peer.trim()}` : 'Starting a new cluster',
        'Firewall opens SSH, 7443/tcp, 7480/tcp',
        'Admin password stored for the web UI',
      ],
    },
    {
      id: 'boot',
      label: 'Finishing boot',
      lines: [
        'UEFI boot entry written',
        'Enabled pertiskd, sshd, firewalld, NetworkManager',
        urls.https ? `Web UI will answer at ${urls.https}` : 'Web UI will answer on port 7443',
      ],
    },
  ]
}
