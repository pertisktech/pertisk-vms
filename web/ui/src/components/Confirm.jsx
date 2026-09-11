import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react'
import { Icon } from './Icons'

const ConfirmCtx = createContext(null)

export function ConfirmProvider({ children }) {
  const [state, setState] = useState(null)
  const resolver = useRef(null)
  const cardRef = useRef(null)
  const backdropDown = useRef(false)

  const confirm = useCallback((opts) => {
    return new Promise((resolve) => {
      resolver.current = resolve
      setState({
        title: opts.title || 'Confirm',
        message: opts.message || 'Are you sure?',
        confirmLabel: opts.confirmLabel || 'Confirm',
        cancelLabel: opts.cancelLabel || 'Cancel',
        tone: opts.tone || 'danger',
      })
    })
  }, [])

  function close(result) {
    setState(null)
    resolver.current?.(result)
    resolver.current = null
  }

  useEffect(() => {
    if (!state) return
    const cancel = cardRef.current?.querySelector('[data-confirm-cancel]')
    cancel?.focus?.()

    function onKeyDown(e) {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        close(false)
        return
      }
      if (e.key === 'Backspace' || e.key === 'Delete') {
        e.preventDefault()
        e.stopPropagation()
      }
    }
    document.addEventListener('keydown', onKeyDown, true)
    return () => document.removeEventListener('keydown', onKeyDown, true)
  }, [state])

  return (
    <ConfirmCtx.Provider value={confirm}>
      {children}
      {state && (
        <div
          className="modal-backdrop confirm-backdrop"
          role="presentation"
          onPointerDown={(e) => {
            backdropDown.current = e.target === e.currentTarget
          }}
          onClick={(e) => {
            if (e.target === e.currentTarget && backdropDown.current) close(false)
            backdropDown.current = false
          }}
        >
          <div
            ref={cardRef}
            className="modal-card"
            role="dialog"
            aria-modal="true"
            aria-labelledby="confirm-title"
            onClick={(e) => e.stopPropagation()}
          >
            <div className={`modal-icon ${state.tone}`}>
              <Icon name={state.tone === 'danger' ? 'alert' : 'check'} size={22} />
            </div>
            <h2 id="confirm-title">{state.title}</h2>
            <p className="muted">{state.message}</p>
            <div className="modal-actions">
              <button type="button" className="secondary" data-confirm-cancel onClick={() => close(false)}>
                {state.cancelLabel}
              </button>
              <button type="button" className={state.tone === 'danger' ? 'danger' : ''} onClick={() => close(true)}>
                {state.confirmLabel}
              </button>
            </div>
          </div>
        </div>
      )}
    </ConfirmCtx.Provider>
  )
}

export function useConfirm() {
  const ctx = useContext(ConfirmCtx)
  if (!ctx) throw new Error('useConfirm requires ConfirmProvider')
  return ctx
}
