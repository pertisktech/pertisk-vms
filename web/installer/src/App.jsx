import { useEffect, useState } from 'react'
import { LuCheck, LuMoon, LuPower, LuSun } from 'react-icons/lu'
import { Mark, cx } from './components'
import {
  STEPS,
  VERSION,
  findDisk,
  firstInvalidStep,
  hasIssues,
  initialForm,
  issues,
} from './model'
import { Access, Disk, Finished, Halt, Installing, Network, Review, Welcome } from './screens'

const SCREENS = {
  welcome: Welcome,
  disk: Disk,
  network: Network,
  access: Access,
  review: Review,
}

const THEME_KEY = 'pertisk-installer-theme'

function readTheme() {
  try {
    return localStorage.getItem(THEME_KEY) === 'light' ? 'light' : 'dark'
  } catch {
    return 'dark'
  }
}

export default function App() {
  const [theme, setTheme] = useState(readTheme)
  const [step, setStep] = useState(0)
  const [form, setForm] = useState(initialForm)
  const [attempted, setAttempted] = useState(false)
  const [phase, setPhase] = useState('wizard')
  const [confirmOff, setConfirmOff] = useState(false)

  useEffect(() => {
    document.documentElement.classList.remove('dark', 'light')
    document.documentElement.classList.add(theme)
    try {
      localStorage.setItem(THEME_KEY, theme)
    } catch {
      /* private mode */
    }
  }, [theme])

  useEffect(() => {
    const title = {
      installing: 'Installing',
      done: 'Ready',
      reboot: 'Rebooting',
      off: 'Powered off',
    }[phase] || STEPS[step].title
    document.title = `${title} · Install Pertisk Vms`
  }, [phase, step])

  const issue = issues(STEPS[step].id, form)
  const Screen = SCREENS[STEPS[step].id]
  const disk = findDisk(form.diskId)

  function reset() {
    setForm(initialForm())
    setStep(0)
    setAttempted(false)
    setPhase('wizard')
    setConfirmOff(false)
  }

  function jump(index) {
    if (phase !== 'wizard' || index > step) return
    setAttempted(false)
    setStep(index)
  }

  function back() {
    if (step === 0) return
    setAttempted(false)
    setStep((current) => current - 1)
  }

  function next() {
    if (STEPS[step].id === 'review') {
      const invalid = firstInvalidStep(form)
      if (invalid >= 0) {
        setStep(invalid)
        setAttempted(true)
        return
      }
      setPhase('installing')
      setAttempted(false)
      return
    }
    if (hasIssues(issue)) {
      setAttempted(true)
      return
    }
    setAttempted(false)
    setStep((current) => current + 1)
  }

  const continueLabel = STEPS[step].id === 'review'
    ? `Install on /dev/${disk?.id || 'disk'}`
    : 'Continue'

  return (
    <div className="shell">
      <aside className="rail">
        <div className="brand">
          <span className="mark"><Mark /></span>
          <div>
            <div className="brand-name">Pertisk <span>Vms</span></div>
            <div className="brand-sub">Node installer</div>
          </div>
        </div>
        <nav aria-label="Installation steps">
          <ol className="step-list">
            {STEPS.map((item, index) => {
              const done = phase !== 'wizard' || index < step
              const current = phase === 'wizard' && index === step
              const state = done ? 'done' : current ? 'current' : 'todo'
              return (
                <li key={item.id}>
                  <button
                    type="button"
                    className={cx('step-btn', state)}
                    aria-current={current ? 'step' : undefined}
                    disabled={phase !== 'wizard' || index > step}
                    onClick={() => jump(index)}
                  >
                    <span className="index">{done ? <LuCheck /> : index + 1}</span>
                    <span className="step-copy">
                      <span className="step-title">{item.title}</span>
                      <small>{item.hint}</small>
                    </span>
                  </button>
                </li>
              )
            })}
          </ol>
        </nav>
        <footer className="rail-foot">
          <button type="button" className="textish" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>
            {theme === 'dark' ? <LuSun /> : <LuMoon />}
            {theme === 'dark' ? 'Light' : 'Dark'}
          </button>
          {phase === 'wizard' && (
            <button type="button" className="textish" onClick={() => setConfirmOff(true)}>
              <LuPower /> Power off
            </button>
          )}
          <p className="rail-meta">Pertisk Vms {VERSION}<br />Preview · sample hardware</p>
        </footer>
      </aside>

      {phase === 'off' || phase === 'reboot' ? (
        <main className="main halt-main">
          {phase === 'off' ? (
            <Halt
              title="Powered off"
              lede="Nothing was written. Boot the installer again when you are ready."
              action="Start again"
              onAction={reset}
            />
          ) : (
            <Halt
              title="Rebooting"
              lede={`${form.hostname} is starting from the installed disk.`}
              action="Back to installer"
              onAction={reset}
            />
          )}
        </main>
      ) : (
        <form
          className="main"
          onSubmit={(event) => {
            event.preventDefault()
            if (phase === 'wizard') next()
          }}
        >
          <div className="main-body">
            {phase === 'wizard' && (
              <Screen
                form={form}
                setForm={setForm}
                showErrors={attempted}
                issue={issue}
                onJump={jump}
              />
            )}
            {phase === 'installing' && (
              <Installing form={form} onComplete={() => setPhase('done')} />
            )}
            {phase === 'done' && (
              <Finished form={form} onReboot={() => setPhase('reboot')} />
            )}
          </div>
          {phase === 'wizard' && (
            <footer className="actions">
              <button type="button" className="secondary" onClick={back} disabled={step === 0}>
                Back
              </button>
              <button type="submit" className="primary">{continueLabel}</button>
            </footer>
          )}
          {phase === 'installing' && (
            <footer className="actions quiet-bar">Do not power off.</footer>
          )}
        </form>
      )}

      {confirmOff && (
        <div className="modal-back" onClick={() => setConfirmOff(false)}>
          <div
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="power-title"
            onClick={(event) => event.stopPropagation()}
          >
            <h2 id="power-title">Power off?</h2>
            <p>Installation has not started. The disks stay as they are.</p>
            <div className="modal-actions">
              <button type="button" className="secondary" onClick={() => setConfirmOff(false)}>Cancel</button>
              <button type="button" className="danger" onClick={() => { setConfirmOff(false); setPhase('off') }}>
                Power off
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
