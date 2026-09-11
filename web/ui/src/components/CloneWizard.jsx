import { useState } from 'react'
import { api, cloudInitHasLogin, detectCloudOs, disksOf, formatBytes, guestMemoryBudgetMib, nextVmId, parseSize } from '../api'
import Modal from './Modal'

const STEPS = [
  { id: 'guest', label: 'Guest' },
  { id: 'net', label: 'Network' },
  { id: 'access', label: 'Access' },
  { id: 'options', label: 'Options' },
]

export default function CloneWizard({ source, vms, volumes, networks, cluster, host, onClose, onCreated }) {
  const budget = guestMemoryBudgetMib(cluster)
  const nodeKeys = (host?.ssh_authorized_keys || []).filter(Boolean).join('\n')
  const detected = detectCloudOs(
    source?.spec?.name,
    ...disksOf(source).map((d) => (volumes || []).find((v) => v.id === d.volume_id)?.name),
  )
  const sourceVol = (volumes || []).find(
    (v) => v.id === disksOf(source).find((d) => !d.cdrom)?.volume_id,
  )
  const minDiskGib = Math.max(1, Math.ceil((Number(sourceVol?.size_bytes) || 8 * 1024 ** 3) / 1024 ** 3))
  const [step, setStep] = useState(0)
  const [userAuto, setUserAuto] = useState(true)
  const [form, setForm] = useState(() => {
    const wanted = Number(source?.spec?.memory_mib) || 1024
    const memory = budget && budget >= 64 ? Math.min(wanted, budget) : wanted
    const diskBytes = Number(
      (volumes || []).find((v) => v.id === disksOf(source).find((d) => !d.cdrom)?.volume_id)
        ?.size_bytes,
    )
    const diskGib = Math.max(1, Math.ceil((diskBytes || 8 * 1024 ** 3) / 1024 ** 3))
    return {
      id: nextVmId(vms),
      name: source?.spec?.name ? `${source.spec.name}-1` : '',
      vcpus: source?.spec?.vcpus || 1,
      memory_mib: memory,
      diskGib,
      ha: true,
      autostart: false,
      linked: false,
      networkId: source?.spec?.nets?.[0]?.network_id || networks[0]?.id || '',
      nicIp: '',
      cloudInit: true,
      ciUser: detected.user,
      ciPassword: '',
      ciSshKey: (host?.ssh_authorized_keys || []).filter(Boolean).join('\n'),
      start: false,
    }
  })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  function set(patch) {
    setForm((f) => ({ ...f, ...patch }))
  }

  const accessOk =
    !form.cloudInit ||
    cloudInitHasLogin(form.ciPassword, form.ciSshKey) ||
    (host?.ssh_authorized_keys || []).length > 0
  const guestOk = /^\d{3,10}$/.test(form.id) && form.name.trim().length > 0

  function canNext() {
    if (step === 0) return guestOk
    if (step === 2) return accessOk
    return true
  }

  function goNext() {
    if (!canNext()) return
    setStep((s) => Math.min(s + 1, STEPS.length - 1))
  }

  async function submit(e) {
    e?.preventDefault?.()
    if (step < STEPS.length - 1) {
      goNext()
      return
    }
    if (!guestOk) {
      setStep(0)
      setError('Set a VM ID and name.')
      return
    }
    if (!accessOk) {
      setStep(2)
      setError('Set a password or an SSH public key. Node keys from /etc/pertisk/ssh/authorized_keys are used automatically.')
      return
    }
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${source.id}/clone`, {
        method: 'POST',
        body: {
          id: Number(form.id),
          name: form.name.trim(),
          linked: form.linked,
          vcpus: Number(form.vcpus),
          memory_mib: Number(form.memory_mib),
          disk_size_bytes: parseSize(`${Math.max(minDiskGib, Number(form.diskGib) || minDiskGib)}G`),
          ha: form.ha,
          autostart: form.autostart,
          network_id: form.networkId || undefined,
          ip: form.nicIp.trim() || undefined,
          cloud_init: form.cloudInit
            ? {
                hostname: form.name.trim(),
                user: (userAuto ? detected.user : form.ciUser.trim()) || detected.user,
                password: form.ciPassword || undefined,
                ssh_authorized_keys: form.ciSshKey
                  .split('\n')
                  .map((s) => s.trim())
                  .filter(Boolean),
              }
            : undefined,
          start: form.start,
        },
      })
      await onCreated()
      onClose()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  const selectedNetwork = networks.find((n) => n.id === form.networkId)
  const sshUser = (userAuto ? detected.user : form.ciUser.trim()) || detected.user

  return (
    <Modal
      title={`Clone ${source?.spec?.name || source?.id || 'template'}`}
      hint="Clones the template disk, injects a cloud-init seed, and defines a new guest."
      wide
      wizard
      onClose={onClose}
      footer={
        <div className="wizard-footer">
          <button type="button" className="secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <div className="wizard-footer-right">
            {step > 0 && (
              <button type="button" className="secondary" onClick={() => setStep((s) => s - 1)} disabled={busy}>
                Back
              </button>
            )}
            {step < STEPS.length - 1 ? (
              <button key="wizard-next" type="button" onClick={goNext} disabled={!canNext()}>
                Next
              </button>
            ) : (
              <button
                key="wizard-clone"
                type="button"
                onClick={submit}
                disabled={busy || !guestOk || !accessOk}
              >
                {busy ? 'Cloning…' : form.start ? 'Clone and start' : 'Clone'}
              </button>
            )}
          </div>
        </div>
      }
    >
      <div className="wizard-steps">
        {STEPS.map((item, i) => (
          <button
            key={item.id}
            type="button"
            className={`wizard-step${i === step ? ' current' : ''}${i < step ? ' done' : ''}`}
            onClick={() => i <= step && setStep(i)}
            disabled={i > step}
          >
            <span className="wizard-step-num">{i + 1}</span>
            <span className="wizard-step-label">{item.label}</span>
          </button>
        ))}
      </div>
      {error && <div className="error">{error}</div>}
      <form id="clone-wizard" onSubmit={submit}>
        {step === 0 && (
          <>
            <p className="wizard-section-title">Machine</p>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="clone-id">VM ID</label>
                <input
                  id="clone-id"
                  required
                  inputMode="numeric"
                  pattern="[0-9]{3,10}"
                  maxLength="10"
                  value={form.id}
                  onChange={(e) => set({ id: e.target.value.replace(/\D/g, '') })}
                />
              </div>
              <div className="field">
                <label htmlFor="clone-name">Name</label>
                <input
                  id="clone-name"
                  required
                  autoFocus
                  value={form.name}
                  onChange={(e) => set({ name: e.target.value })}
                />
              </div>
            </div>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="clone-cpu">vCPU</label>
                <input
                  id="clone-cpu"
                  type="number"
                  min="1"
                  value={form.vcpus}
                  onChange={(e) => set({ vcpus: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="clone-mem">Memory (MiB)</label>
                <input
                  id="clone-mem"
                  type="number"
                  min="64"
                  step="64"
                  value={form.memory_mib}
                  onChange={(e) => set({ memory_mib: e.target.value })}
                />
                {budget != null && (
                  <p className="field-hint">
                    This node can start guests up to {budget} MiB. Clone still works if you ask for more; start later after
                    lowering memory.
                  </p>
                )}
              </div>
            </div>
            <div className="field">
              <label htmlFor="clone-disk">Disk (GiB)</label>
              <input
                id="clone-disk"
                type="number"
                min={minDiskGib}
                step="1"
                value={form.diskGib}
                onChange={(e) => set({ diskGib: e.target.value })}
              />
              <p className="field-hint">
                Template disk is {sourceVol ? formatBytes(sourceVol.size_bytes) : `${minDiskGib} GiB`}.
                You can grow it; cloud-init expands the partition on first boot. Shrinking is not supported.
              </p>
            </div>
          </>
        )}

        {step === 1 && (
          <>
            <p className="wizard-section-title">Network</p>
            <div className="field">
              <label htmlFor="clone-net">Network</label>
              <select id="clone-net" value={form.networkId} onChange={(e) => set({ networkId: e.target.value })}>
                <option value="">None</option>
                {networks.map((n) => (
                  <option key={n.id} value={n.id}>
                    {n.name} ({n.cidr})
                  </option>
                ))}
              </select>
            </div>
            {form.networkId && (
              <div className="field">
                <label htmlFor="clone-ip">Static IP</label>
                <input
                  id="clone-ip"
                  value={form.nicIp}
                  onChange={(e) => set({ nicIp: e.target.value })}
                  placeholder={
                    selectedNetwork?.gateway
                      ? `DHCP; gateway ${selectedNetwork.gateway} is reserved`
                      : 'leave empty for DHCP'
                  }
                />
              </div>
            )}
          </>
        )}

        {step === 2 && (
          <>
            <p className="wizard-section-title">Cloud-init</p>
            <label className="chk">
              <input
                type="checkbox"
                checked={form.cloudInit}
                onChange={(e) => set({ cloudInit: e.target.checked })}
              />
              <span className="chk-box" />
              <span className="chk-label">
                Cloud-init seed
                <small>Hostname, user, password, and SSH keys for a cloud image</small>
              </span>
            </label>
            {form.cloudInit && (
              <>
                <div className="form-grid" style={{ marginTop: '1rem' }}>
                  <div className="field">
                    <label htmlFor="clone-ci-user">User</label>
                    <input
                      id="clone-ci-user"
                      value={userAuto ? detected.user : form.ciUser}
                      onChange={(e) => {
                        setUserAuto(false)
                        set({ ciUser: e.target.value })
                      }}
                    />
                    <p className="field-hint">
                      Auto from {detected.os}: <code>{detected.user}</code>. SSH as{' '}
                      <code>
                        {sshUser}@…
                      </code>
                      {!userAuto && form.ciUser.trim() !== detected.user && (
                        <>
                          {' '}
                          <button
                            type="button"
                            className="secondary"
                            style={{ padding: '0.1rem 0.45rem', fontSize: '0.75rem' }}
                            onClick={() => {
                              setUserAuto(true)
                              set({ ciUser: detected.user })
                            }}
                          >
                            Reset
                          </button>
                        </>
                      )}
                    </p>
                  </div>
                  <div className="field">
                    <label htmlFor="clone-ci-pass">Password</label>
                    <input
                      id="clone-ci-pass"
                      type="password"
                      value={form.ciPassword}
                      onChange={(e) => set({ ciPassword: e.target.value })}
                      autoComplete="new-password"
                    />
                  </div>
                </div>
                <div className="field">
                  <label htmlFor="clone-ci-ssh">SSH authorized keys</label>
                  <textarea
                    id="clone-ci-ssh"
                    rows={4}
                    value={form.ciSshKey}
                    onChange={(e) => set({ ciSshKey: e.target.value })}
                    placeholder="ssh-ed25519 AAAA… (one per line)"
                  />
                  <p className="field-hint">
                    Paste one public key per line, or leave this empty if the node already has keys in{' '}
                    <code>/etc/pertisk/ssh/authorized_keys</code>. Same as a normal cloud VM: <code>ssh {sshUser}@…</code>{' '}
                    with your key. A password is optional; Pertisk never expires it.
                  </p>
                </div>
                {!accessOk && (
                  <p className="error" style={{ marginTop: '0.5rem' }}>
                    Set a password or paste an SSH public key.
                  </p>
                )}
              </>
            )}
          </>
        )}

        {step === 3 && (
          <>
            <p className="wizard-section-title">Options</p>
            <div className="wizard-options">
              <label className="chk">
                <input type="checkbox" checked={form.linked} onChange={(e) => set({ linked: e.target.checked })} />
                <span className="chk-box" />
                <span className="chk-label">
                  Linked clone
                  <small>
                    Thin overlay on the template. Off by default for cloud images (AlmaLinux/RHEL XFS needs a full
                    uncompressed disk).
                  </small>
                </span>
              </label>
              <label className="chk">
                <input type="checkbox" checked={form.ha} onChange={(e) => set({ ha: e.target.checked })} />
                <span className="chk-box" />
                <span className="chk-label">
                  Restart on another node if this one is lost
                  <small>High availability</small>
                </span>
              </label>
              <label className="chk">
                <input
                  type="checkbox"
                  checked={form.autostart}
                  onChange={(e) => set({ autostart: e.target.checked })}
                />
                <span className="chk-box" />
                <span className="chk-label">
                  Start at boot
                  <small>Power on when this node starts</small>
                </span>
              </label>
              <label className="chk">
                <input type="checkbox" checked={form.start} onChange={(e) => set({ start: e.target.checked })} />
                <span className="chk-box" />
                <span className="chk-label">
                  Start after clone
                  <small>Boot the guest as soon as it is defined. Needs {form.memory_mib} MiB free for guests.</small>
                </span>
              </label>
            </div>
          </>
        )}
      </form>
    </Modal>
  )
}
