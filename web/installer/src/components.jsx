import { useEffect, useMemo, useRef, useState } from 'react'
import { LuChevronDown, LuEye, LuEyeOff } from 'react-icons/lu'
import { listTimezones } from './model'

export function cx(...parts) {
  return parts.filter(Boolean).join(' ')
}

export function Mark() {
  return (
    <svg className="mark-svg" viewBox="0 0 32 32" aria-hidden="true">
      <rect width="32" height="32" rx="8" fill="currentColor" />
      <rect x="7" y="8" width="18" height="3.5" rx="1" fill="#d6bef5" />
      <rect x="7" y="14.25" width="18" height="3.5" rx="1" fill="#8a4ed6" />
      <rect x="7" y="20.5" width="12" height="3.5" rx="1" fill="#5a2da0" />
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

export function TimezoneField({ id, label, value, onChange }) {
  const zones = useMemo(() => listTimezones(), [])
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState(value)
  const [active, setActive] = useState(-1)
  const rootRef = useRef(null)
  const listRef = useRef(null)
  const listId = `${id}-list`

  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase().replace(/\s+/g, '_')
    const selected = value.trim().toLowerCase().replace(/\s+/g, '_')
    // Keep the selected zone in the field, but show every zone until the
    // operator types a different search string.
    if (!needle || needle === selected) return zones
    return zones.filter((zone) => zone.toLowerCase().includes(needle))
  }, [query, value, zones])

  useEffect(() => {
    if (!open) setQuery(value)
  }, [value, open])

  useEffect(() => {
    if (!open) return
    function onDoc(event) {
      if (!rootRef.current?.contains(event.target)) setOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    return () => document.removeEventListener('mousedown', onDoc)
  }, [open])

  useEffect(() => {
    if (!open || active < 0) return
    const node = listRef.current?.children?.[active]
    node?.scrollIntoView?.({ block: 'nearest' })
  }, [active, open])

  function pick(zone) {
    onChange(zone)
    setQuery(zone)
    setOpen(false)
    setActive(-1)
  }

  function onKeyDown(event) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setOpen(true)
      setActive((i) => Math.min((i < 0 ? -1 : i) + 1, matches.length - 1))
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      setOpen(true)
      setActive((i) => Math.max((i < 0 ? matches.length : i) - 1, 0))
      return
    }
    if (event.key === 'Enter') {
      if (open && matches[active]) {
        event.preventDefault()
        pick(matches[active])
      } else if (open && matches.length === 1) {
        event.preventDefault()
        pick(matches[0])
      }
      return
    }
    if (event.key === 'Escape') {
      setOpen(false)
      setQuery(value)
      setActive(-1)
    }
  }

  return (
    <Field id={id} label={label}>
      <div className={cx('combo', open && 'open')} ref={rootRef}>
        <input
          id={id}
          role="combobox"
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={active >= 0 ? `${id}-opt-${active}` : undefined}
          value={query}
          spellCheck={false}
          autoComplete="off"
          placeholder="Search time zones"
          onChange={(e) => {
            setQuery(e.target.value)
            setOpen(true)
            setActive(0)
          }}
          onFocus={() => {
            setOpen(true)
            setActive(Math.max(0, matches.indexOf(value)))
          }}
          onKeyDown={onKeyDown}
        />
        <button
          type="button"
          className="combo-toggle"
          tabIndex={-1}
          aria-label="Show time zones"
          onClick={() => {
            setOpen((was) => !was)
            setActive(Math.max(0, matches.indexOf(value)))
          }}
        >
          <LuChevronDown />
        </button>
        {open ? (
          <ul id={listId} className="combo-list" role="listbox" ref={listRef}>
            {matches.length === 0 ? (
              <li className="combo-empty" role="presentation">No matches</li>
            ) : (
              matches.map((zone, index) => (
                <li
                  key={zone}
                  id={`${id}-opt-${index}`}
                  role="option"
                  aria-selected={zone === value}
                  className={cx(zone === value && 'selected', index === active && 'active')}
                  onMouseEnter={() => setActive(index)}
                  onMouseDown={(event) => {
                    event.preventDefault()
                    pick(zone)
                  }}
                >
                  {zone}
                </li>
              ))
            )}
          </ul>
        ) : null}
      </div>
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
