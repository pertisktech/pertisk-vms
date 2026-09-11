import { useEffect, useRef } from 'react'

function isEditable(el) {
  if (!(el instanceof HTMLElement)) return false
  if (el.isContentEditable) return true
  const tag = el.tagName
  if (tag === 'TEXTAREA' || tag === 'SELECT') return true
  if (tag !== 'INPUT') return false
  const type = (el.getAttribute('type') || 'text').toLowerCase()
  return !['button', 'submit', 'reset', 'checkbox', 'radio', 'file', 'range', 'color', 'hidden'].includes(type)
}

function focusables(root) {
  if (!root) return []
  return [...root.querySelectorAll(
    'input:not([type="hidden"]):not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
  )].filter((el) => el instanceof HTMLElement && el.offsetParent !== null)
}

export default function Modal({ title, hint, wide, wizard, lock, onClose, children, footer }) {
  const cardRef = useRef(null)
  const onCloseRef = useRef(onClose)
  const backdropDown = useRef(false)
  onCloseRef.current = onClose
  const locked = lock ?? true

  useEffect(() => {
    const card = cardRef.current
    const prev = document.activeElement
    const nodes = focusables(card).filter((el) => !el.classList.contains('modal-close'))
    const initial = nodes.find(isEditable) || nodes[0]
    initial?.focus?.()

    function onKeyDown(e) {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        onCloseRef.current?.()
        return
      }
      if ((e.key === 'Backspace' || e.key === 'Delete') && !isEditable(e.target)) {
        e.preventDefault()
        e.stopPropagation()
        return
      }
      if (e.key !== 'Tab' || !card) return
      const list = focusables(card)
      if (!list.length) return
      const first = list[0]
      const last = list[list.length - 1]
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
    }

    document.addEventListener('keydown', onKeyDown, true)
    return () => {
      document.removeEventListener('keydown', onKeyDown, true)
      if (prev instanceof HTMLElement) prev.focus?.()
    }
  }, [])

  function onBackdropPointerDown(e) {
    backdropDown.current = !locked && e.target === e.currentTarget
  }

  function onBackdropClick(e) {
    if (!locked && e.target === e.currentTarget && backdropDown.current) {
      onCloseRef.current?.()
    }
    backdropDown.current = false
  }

  return (
    <div
      className={`modal-backdrop${wizard ? ' wizard-backdrop' : ''}${locked ? ' modal-locked' : ''}`}
      role="presentation"
      onPointerDown={onBackdropPointerDown}
      onClick={onBackdropClick}
    >
      <div
        ref={cardRef}
        className={`modal-card${wide ? ' modal-wide' : ''}${wizard ? ' modal-wizard' : ''}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <h2 id="modal-title">{title}</h2>
          <button type="button" className="secondary modal-close" onClick={() => onCloseRef.current?.()} aria-label="Close">
            ×
          </button>
        </div>
        {hint && <p className="modal-hint">{hint}</p>}
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-actions">{footer}</div>}
      </div>
    </div>
  )
}
