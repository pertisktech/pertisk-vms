import { useEffect, useMemo, useRef, useState } from 'react'
import { LuCheck, LuCopy, LuTriangleAlert } from 'react-icons/lu'
import { Banner, Check, Field, PasswordField, SelectField, StepHeader, TimezoneField, cx } from './components'
import {
  KEYBOARDS,
  LANGUAGES,
  diskList,
  findDisk,
  findNic,
  formatBytes,
  hostInfo,
  installScript,
  keyboardLabel,
  languageLabel,
  managementAddress,
  nicList,
  partitionPlan,
  passwordLabel,
  passwordScore,
  uiUrls,
} from './model'

function patch(setForm, partial) {
  setForm((form) => ({ ...form, ...partial }))
}

export function Welcome({ form, setForm, live }) {
  const host = hostInfo()
  return (
    <div className="step">
      <StepHeader
        title="Install Pertisk Vms"
        lede="AlmaLinux 10 and the virtualization control plane are written to the disk you choose. Secure Boot stays off."
      />
      <dl className="facts">
        <div>
          <dt>Processor</dt>
          <dd>{host.cpu} · {host.threads} threads</dd>
        </div>
        <div>
          <dt>Memory</dt>
          <dd>{host.memory}</dd>
        </div>
        <div>
          <dt>Firmware</dt>
          <dd>{host.firmware} · Secure Boot {host.secureBoot ? 'on' : 'off'}</dd>
        </div>
        <div>
          <dt>Virtualization</dt>
          <dd>{host.virt}</dd>
        </div>
      </dl>
      {live ? null : (
        <p className="quiet">Sample machine for this preview. A live install reads the hardware you boot.</p>
      )}
      <div className="split">
        <SelectField id="lang" label="Language" value={form.lang} onChange={(lang) => patch(setForm, { lang })}>
          {LANGUAGES.map((lang) => (
            <option key={lang.id} value={lang.id}>{lang.label}</option>
          ))}
        </SelectField>
        <SelectField id="keyboard" label="Keyboard" value={form.keyboard} onChange={(keyboard) => patch(setForm, { keyboard })}>
          {KEYBOARDS.map((board) => (
            <option key={board.id} value={board.id}>{board.label}</option>
          ))}
        </SelectField>
        <div className="wide">
          <TimezoneField
            id="timezone"
            label="Time zone"
            value={form.timezone}
            onChange={(timezone) => patch(setForm, { timezone })}
          />
        </div>
      </div>
      <p className="note">Packages come from the AlmaLinux mirrors, so this machine needs a network connection during the install.</p>
    </div>
  )
}

export function Disk({ form, setForm, showErrors, issue }) {
  const disk = findDisk(form.diskId)
  const plan = disk && disk.kind === 'disk' ? partitionPlan(disk.size) : []
  const eraseError = showErrors ? issue.fields.erase : ''

  function selectDisk(id) {
    setForm((current) => ({
      ...current,
      diskId: id,
      eraseConfirmed: current.diskId === id ? current.eraseConfirmed : false,
    }))
  }

  return (
    <div className="step">
      <StepHeader
        title="Installation disk"
        lede="Guest images live on /var/lib/pertisk. The system volume stays small so the rest of the disk is for VMs."
      />
      {showErrors && issue.formError ? <Banner tone="bad">{issue.formError}</Banner> : null}
      <div className="choices" role="radiogroup" aria-label="Target disk">
        {diskList().map((item) => {
          const selected = item.id === form.diskId
          const blocked = item.kind !== 'disk'
          return (
            <button
              key={item.id}
              type="button"
              role="radio"
              aria-checked={selected}
              className={cx('choice', selected && 'selected', blocked && 'blocked')}
              disabled={blocked}
              onClick={() => selectDisk(item.id)}
            >
              <span className="choice-main">
                <span className="choice-title">{item.model}</span>
                <span className="choice-meta">
                  {item.bus} · /dev/{item.id}
                  {blocked ? ' · Installer USB' : ''}
                </span>
              </span>
              <span className="choice-size">{formatBytes(item.size)}</span>
            </button>
          )
        })}
      </div>
      {plan.length > 0 && (
        <div className="layout">
          <div className="layout-label">
            <span>Proposed layout</span>
            <span>Root is capped at 70 GiB</span>
          </div>
          <div className="layout-bar" aria-hidden="true">
            {plan.map((part) => (
              <span
                key={part.id}
                className={`seg seg-${part.id}`}
                style={{ flexGrow: Math.max(part.bytes, disk.size * 0.035) }}
                title={`${part.mount} ${formatBytes(part.bytes)}`}
              />
            ))}
          </div>
          <ul className="legend">
            {plan.map((part) => (
              <li key={part.id}>
                <i className={`seg-${part.id}`} />
                <span>{part.mount}</span>
                <strong>{formatBytes(part.bytes)}</strong>
              </li>
            ))}
          </ul>
        </div>
      )}
      <Check
        className={cx('confirm', eraseError && 'bad')}
        checked={form.eraseConfirmed}
        onChange={(eraseConfirmed) => patch(setForm, { eraseConfirmed })}
      >
        <strong>Erase /dev/{disk?.id}</strong>
        <small>
          {disk ? `${disk.model} · ${formatBytes(disk.size)}. This cannot be undone.` : 'Select a disk.'}
        </small>
        {eraseError ? <small className="field-error">{eraseError}</small> : null}
      </Check>
    </div>
  )
}

export function Network({ form, setForm, showErrors, issue }) {
  const nic = findNic(form.nicId)
  const urls = uiUrls(form)
  const field = (name) => (showErrors ? issue.fields[name] : '')

  return (
    <div className="step">
      <StepHeader
        title="Network"
        lede="The web UI listens on this address after reboot. HTTPS is 7443. HTTP is 7480."
      />
      {showErrors && issue.formError ? <Banner tone="bad">{issue.formError}</Banner> : null}
      <div className="choices" role="radiogroup" aria-label="Management interface">
        {nicList().map((item) => {
          const selected = item.id === form.nicId
          return (
            <button
              key={item.id}
              type="button"
              role="radio"
              aria-checked={selected}
              className={cx('choice', selected && 'selected')}
              onClick={() => patch(setForm, { nicId: item.id })}
            >
              <span className={cx('link-dot', item.link && 'up')} aria-hidden="true" />
              <span className="choice-main">
                <span className="choice-title">{item.id}</span>
                <span className="choice-meta">
                  {item.model} · {item.mac}
                  {item.link ? ` · ${item.speed}` : ' · no link'}
                </span>
              </span>
              <span className="choice-size">{item.link ? 'Up' : 'Down'}</span>
            </button>
          )
        })}
      </div>
      {nic && !nic.link ? (
        <Banner>No link on {nic.id}. DHCP waits until a cable is connected.</Banner>
      ) : null}
      <div className="segmented" role="radiogroup" aria-label="IPv4 mode">
        {[
          ['dhcp', 'DHCP'],
          ['static', 'Static'],
        ].map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="radio"
            aria-checked={form.mode === id}
            className={form.mode === id ? 'on' : ''}
            onClick={() => patch(setForm, { mode: id })}
          >
            {label}
          </button>
        ))}
      </div>
      {form.mode === 'static' && (
        <>
          <div className="addr-row">
            <Field id="address" label="Address" error={field('address')}>
              <input
                id="address"
                value={form.address}
                inputMode="decimal"
                spellCheck={false}
                autoComplete="off"
                aria-invalid={field('address') ? true : undefined}
                onChange={(e) => patch(setForm, { address: e.target.value })}
              />
            </Field>
            <Field id="prefix" label="Prefix" error={field('prefix')}>
              <input
                id="prefix"
                value={form.prefix}
                inputMode="numeric"
                aria-invalid={field('prefix') ? true : undefined}
                onChange={(e) => patch(setForm, { prefix: e.target.value })}
              />
            </Field>
          </div>
          <div className="split">
            <Field id="gateway" label="Gateway" error={field('gateway')}>
              <input
                id="gateway"
                value={form.gateway}
                inputMode="decimal"
                spellCheck={false}
                autoComplete="off"
                aria-invalid={field('gateway') ? true : undefined}
                onChange={(e) => patch(setForm, { gateway: e.target.value })}
              />
            </Field>
            <Field id="dns" label="DNS" error={field('dns')}>
              <input
                id="dns"
                value={form.dns}
                inputMode="decimal"
                spellCheck={false}
                autoComplete="off"
                aria-invalid={field('dns') ? true : undefined}
                onChange={(e) => patch(setForm, { dns: e.target.value })}
              />
            </Field>
          </div>
        </>
      )}
      <Field
        id="hostname"
        label="Hostname"
        hint="Short name for this node. Not a domain."
        error={field('hostname')}
      >
        <input
          id="hostname"
          value={form.hostname}
          spellCheck={false}
          autoComplete="off"
          aria-invalid={field('hostname') ? true : undefined}
          aria-describedby={field('hostname') ? 'hostname-error' : undefined}
          onChange={(e) => patch(setForm, { hostname: e.target.value })}
        />
      </Field>
      <aside className="preview">
        <span>After reboot</span>
        {urls.https ? (
          <>
            <strong>{urls.https}</strong>
            <span>{urls.http}</span>
          </>
        ) : (
          <strong>No address yet</strong>
        )}
      </aside>
      <details className="fold">
        <summary>Join a cluster</summary>
        <Field
          id="peer"
          label="Peer URL"
          hint="Leave empty to start a new cluster."
          error={field('peer')}
        >
          <input
            id="peer"
            value={form.peer}
            placeholder="https://10.1.1.16:7443"
            spellCheck={false}
            autoComplete="off"
            aria-invalid={field('peer') ? true : undefined}
            onChange={(e) => patch(setForm, { peer: e.target.value })}
          />
        </Field>
      </details>
    </div>
  )
}

export function Access({ form, setForm, showErrors, issue }) {
  const field = (name) => (showErrors ? issue.fields[name] : '')
  const score = passwordScore(form.adminPassword)
  const label = passwordLabel(form.adminPassword)

  return (
    <div className="step">
      <StepHeader
        title="Administrator"
        lede="One password for the web UI, SSH, and the local console. The user is admin."
      />
      <div className="identity">
        <span>User</span>
        <strong>admin</strong>
      </div>
      <PasswordField
        id="adminPassword"
        label="Password"
        value={form.adminPassword}
        error={field('adminPassword')}
        autoFocus
        onChange={(adminPassword) => patch(setForm, { adminPassword })}
      />
      {form.adminPassword ? (
        <div className="meter">
          <span className={`meter-bar score-${score}`}>
            <i /><i /><i /><i />
          </span>
          <span>{label}</span>
        </div>
      ) : null}
      <PasswordField
        id="adminConfirm"
        label="Confirm password"
        value={form.adminConfirm}
        error={field('adminConfirm')}
        onChange={(adminConfirm) => patch(setForm, { adminConfirm })}
      />
      <Check
        className="check"
        checked={form.separateRoot}
        onChange={(separateRoot) => patch(setForm, { separateRoot })}
      >
        Use a different root password for the console
      </Check>
      {form.separateRoot && (
        <div className="split">
          <PasswordField
            id="rootPassword"
            label="Root password"
            value={form.rootPassword}
            error={field('rootPassword')}
            onChange={(rootPassword) => patch(setForm, { rootPassword })}
          />
          <PasswordField
            id="rootConfirm"
            label="Confirm root password"
            value={form.rootConfirm}
            error={field('rootConfirm')}
            onChange={(rootConfirm) => patch(setForm, { rootConfirm })}
          />
        </div>
      )}
    </div>
  )
}

function ReviewBlock({ title, onChange, children }) {
  return (
    <section className="review-block">
      <header>
        <h2>{title}</h2>
        {onChange ? (
          <button type="button" className="linkish" onClick={onChange}>Change</button>
        ) : null}
      </header>
      <dl>{children}</dl>
    </section>
  )
}

function Row({ k, v }) {
  return (
    <div>
      <dt>{k}</dt>
      <dd>{v}</dd>
    </div>
  )
}

export function Review({ form, onJump }) {
  const disk = findDisk(form.diskId)
  const nic = findNic(form.nicId)
  const address = managementAddress(form)
  const plan = disk ? partitionPlan(disk.size) : []

  return (
    <div className="step">
      <StepHeader
        title="Review"
        lede="Installation erases the selected disk, then reboots into the node."
      />
      <Banner>
        <LuTriangleAlert />
        <span>Everything on /dev/{disk?.id} will be deleted.</span>
      </Banner>
      <ReviewBlock title="Machine">
        <Row k="Processor" v={`${hostInfo().cpu} · ${hostInfo().threads} threads · ${hostInfo().memory}`} />
        <Row k="Firmware" v={`${hostInfo().firmware} · Secure Boot ${hostInfo().secureBoot ? 'on' : 'off'}`} />
      </ReviewBlock>
      <ReviewBlock title="Disk" onChange={() => onJump(1)}>
        <Row k="Target" v={`/dev/${disk?.id} · ${disk?.model}`} />
        <Row k="Size" v={disk ? formatBytes(disk.size) : ''} />
        <Row k="Layout" v={plan.map((part) => part.mount).join(', ')} />
      </ReviewBlock>
      <ReviewBlock title="Network" onChange={() => onJump(2)}>
        <Row k="Interface" v={nic ? `${nic.id} · ${nic.model}` : ''} />
        <Row
          k="IPv4"
          v={form.mode === 'dhcp'
            ? `DHCP${address ? ` · ${address}` : ''}`
            : `${form.address}/${form.prefix}`}
        />
        <Row k="Hostname" v={form.hostname} />
        <Row k="Cluster" v={form.peer.trim() || 'New cluster'} />
      </ReviewBlock>
      <ReviewBlock title="Access" onChange={() => onJump(3)}>
        <Row k="Web UI and SSH" v="admin" />
        <Row k="Console root" v={form.separateRoot ? 'Separate password' : 'Same password'} />
      </ReviewBlock>
      <ReviewBlock title="Locale" onChange={() => onJump(0)}>
        <Row k="Language" v={languageLabel(form.lang)} />
        <Row k="Keyboard" v={keyboardLabel(form.keyboard)} />
        <Row k="Time zone" v={form.timezone} />
      </ReviewBlock>
    </div>
  )
}

const STAGE_MS = [1500, 2200, 1400, 1600, 1300]

export function Installing({ form, onComplete, live }) {
  const script = useMemo(() => installScript(form), [form])
  const [pct, setPct] = useState(0)
  const [stage, setStage] = useState(0)
  const [lines, setLines] = useState([])
  const [error, setError] = useState('')
  const logRef = useRef(null)
  const onCompleteRef = useRef(onComplete)
  onCompleteRef.current = onComplete

  useEffect(() => {
    if (!live) return undefined
    let stop = false
    let timer = 0
    let misses = 0
    const poll = async () => {
      try {
        const res = await fetch('/api/progress')
        const data = await res.json()
        if (stop) return
        misses = 0
        setError('')
        setPct(data.pct || 0)
        setStage(data.stage || 0)
        setLines(data.lines || [])
        if (data.status === 'done') {
          onCompleteRef.current()
          return
        }
        if (data.status === 'error') {
          setError(data.error || 'Installation failed.')
          return
        }
      } catch {
        if (stop) return
        misses += 1
        if (misses >= 5) setError('Reconnecting to the installer…')
      }
      timer = window.setTimeout(poll, 1000)
    }
    poll()
    return () => {
      stop = true
      clearTimeout(timer)
    }
  }, [live])

  useEffect(() => {
    if (live) return undefined
    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const scale = reduce ? 0.2 : 1
    const events = []
    let at = 0
    script.forEach((item, index) => {
      const duration = STAGE_MS[index] * scale
      events.push({ at, type: 'stage', index })
      item.lines.forEach((text, lineIndex) => {
        events.push({
          at: at + ((lineIndex + 1) / (item.lines.length + 1)) * duration,
          type: 'line',
          text,
        })
      })
      at += duration
    })
    const total = at
    const started = performance.now()
    let raf = 0
    let doneTimer = 0

    const tick = (now) => {
      const elapsed = now - started
      const visible = events.filter((event) => event.at <= elapsed)
      const current = [...visible].reverse().find((event) => event.type === 'stage')
      setPct(Math.min(100, (elapsed / total) * 100))
      setStage(current ? current.index : 0)
      setLines(visible.filter((event) => event.type === 'line').map((event) => event.text))
      if (elapsed >= total) {
        setPct(100)
        setStage(script.length - 1)
        doneTimer = window.setTimeout(() => onCompleteRef.current(), 500)
        return
      }
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => {
      cancelAnimationFrame(raf)
      clearTimeout(doneTimer)
    }
  }, [script, live])

  useEffect(() => {
    const node = logRef.current
    if (node) node.scrollTop = node.scrollHeight
  }, [lines])

  return (
    <div className="step install-step">
      <StepHeader
        title="Installing"
        lede="Do not power off. The node reboots only when you ask it to."
      />
      {error ? <Banner tone="bad">{error}</Banner> : null}
      <div className="progress-head">
        <strong>{Math.round(pct)}%</strong>
        <span>{script[stage]?.label}</span>
      </div>
      <div className="progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)} role="progressbar">
        <span style={{ width: `${pct}%` }} />
      </div>
      <div className="install-grid">
        <ol className="stage-list">
          {script.map((item, index) => (
            <li key={item.id} className={index < stage ? 'done' : index === stage ? 'current' : ''}>
              <span>{index < stage ? <LuCheck /> : index + 1}</span>
              {item.label}
            </li>
          ))}
        </ol>
        <pre ref={logRef} className="log" aria-live="polite">
          {lines.map((line, index) => (
            <div key={`${index}-${line}`}>{line}</div>
          ))}
        </pre>
      </div>
    </div>
  )
}

function copyViaKiosk(text) {
  const handler = window.webkit?.messageHandlers?.pertiskCopy
  if (!handler || typeof handler.postMessage !== 'function') return false
  handler.postMessage(text)
  return true
}

function copyViaSelection(text) {
  const area = document.createElement('textarea')
  area.value = text
  area.setAttribute('readonly', '')
  area.setAttribute('aria-hidden', 'true')
  area.style.position = 'fixed'
  area.style.top = '0'
  area.style.left = '-9999px'
  document.body.appendChild(area)
  area.focus()
  area.select()
  let ok = false
  try {
    ok = document.execCommand('copy')
  } catch {
    ok = false
  }
  area.remove()
  return ok
}

async function writeClipboard(text) {
  // The kiosk bridge and execCommand have to run inside the click.
  // navigator.clipboard.writeText often never settles in WebKit.
  if (copyViaKiosk(text) || copyViaSelection(text)) return true
  if (!navigator.clipboard?.writeText || !window.isSecureContext) return false
  try {
    return await Promise.race([
      navigator.clipboard.writeText(text).then(() => true, () => false),
      new Promise((resolve) => window.setTimeout(() => resolve(false), 200)),
    ])
  } catch {
    return false
  }
}

export function Finished({ form, onReboot }) {
  const urls = uiUrls(form)
  const urlRef = useRef(null)
  const copyTimer = useRef(0)
  const [copyState, setCopyState] = useState('idle')

  useEffect(() => () => window.clearTimeout(copyTimer.current), [])

  async function copy() {
    const text = urls.https
    if (!text) return
    const ok = await writeClipboard(text)
    if (!ok && urlRef.current) {
      const range = document.createRange()
      range.selectNodeContents(urlRef.current)
      const selection = window.getSelection()
      selection?.removeAllRanges()
      selection?.addRange(range)
    }
    setCopyState(ok ? 'copied' : 'failed')
    window.clearTimeout(copyTimer.current)
    copyTimer.current = window.setTimeout(() => setCopyState('idle'), 2000)
  }

  const copyLabel = copyState === 'copied' ? 'Copied' : copyState === 'failed' ? 'Copy failed' : 'Copy URL'

  return (
    <div className="step">
      <StepHeader
        title="Ready to reboot"
        lede="pertiskd starts on the next boot. Sign in as admin with the password you set."
      />
      <div className="done-card">
        <span className="done-kicker">Open</span>
        {urls.https ? <strong ref={urlRef} className="url">{urls.https}</strong> : <strong className="url">https://&lt;node&gt;:7443/</strong>}
        <span className="url-alt">{urls.http || 'http://<node>:7480/'}</span>
        <dl>
          <div>
            <dt>User</dt>
            <dd>admin</dd>
          </div>
          <div>
            <dt>Hostname</dt>
            <dd>{form.hostname}</dd>
          </div>
        </dl>
        {urls.https ? (
          <>
            <button type="button" className="secondary copy" onClick={copy}>
              <LuCopy /> {copyLabel}
            </button>
            {copyState === 'failed' ? (
              <p className="quiet">The address is selected. Press Ctrl+C.</p>
            ) : null}
          </>
        ) : (
          <p className="quiet">No address yet. Check the cable, then look at the console for the IP.</p>
        )}
      </div>
      <button type="button" className="primary reboot" onClick={onReboot}>
        Reboot
      </button>
    </div>
  )
}

export function Halt({ title, lede, action, onAction }) {
  return (
    <div className="halt">
      <h1>{title}</h1>
      <p>{lede}</p>
      {action ? (
        <button type="button" className="secondary" onClick={onAction}>{action}</button>
      ) : null}
    </div>
  )
}
