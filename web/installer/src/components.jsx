import { useState } from 'react'
import { LuEye, LuEyeOff } from 'react-icons/lu'

export function cx(...parts) {
  return parts.filter(Boolean).join(' ')
}

export function Mark() {
  return (
    <svg className="mark-svg" viewBox="0 0 32 32" aria-hidden="true">
      <rect width="32" height="32" rx="8" fill="currentColor" />
      <rect x="7" y="8" width="18" height="3.5" rx="1" fill="#8ed4cc" />
      <rect x="7" y="14.25" width="18" height="3.5" rx="1" fill="#2eaaa0" />
      <rect x="7" y="20.5" width="12" height="3.5" rx="1" fill="#14756e" />
    </svg>
  )
}

export function Field({ id, label, hint, error, children }) {
  return (
    <div className={cx('field', error && 'invalid')}>
      <label htmlFor={id}>{label}</label>
      {children}
      {error ? (
        <p className="field-error" id={`${id}-error`}>{error}</p>
      ) : hint ? (
        <p className="field-hint">{hint}</p>
      ) : null}
    </div>
  )
}

export function PasswordField({ id, label, value, onChange, error, hint, autoFocus }) {
  const [show, setShow] = useState(false)
  return (
    <Field id={id} label={label} error={error} hint={hint}>
      <div className="secret">
        <input
          id={id}
          type={show ? 'text' : 'password'}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          autoComplete="new-password"
          autoFocus={autoFocus}
          spellCheck={false}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? `${id}-error` : undefined}
        />
        <button
          type="button"
          className="ghost-icon"
          onClick={() => setShow((open) => !open)}
          aria-label={show ? 'Hide password' : 'Show password'}
        >
          {show ? <LuEyeOff /> : <LuEye />}
        </button>
      </div>
    </Field>
  )
}

export function SelectField({ id, label, value, onChange, children }) {
  return (
    <Field id={id} label={label}>
      <select id={id} value={value} onChange={(e) => onChange(e.target.value)}>
        {children}
      </select>
    </Field>
  )
}

export function Check({ className, checked, onChange, children }) {
  return (
    <button
      type="button"
      className={cx(className, checked && 'on')}
      role="checkbox"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    >
      <span className={cx('tick', checked && 'on')} aria-hidden="true" />
      <span className="check-copy">{children}</span>
    </button>
  )
}

export function Banner({ tone = 'warn', children }) {
  return (
    <div className={cx('banner', tone)} role={tone === 'bad' ? 'alert' : 'note'}>
      {children}
    </div>
  )
}

export function StepHeader({ title, lede }) {
  return (
    <header className="step-head">
      <h1>{title}</h1>
      <p>{lede}</p>
    </header>
  )
}
