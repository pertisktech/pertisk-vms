export default function Modal({ title, hint, wide, wizard, onClose, children, footer }) {
  return (
    <div className={`modal-backdrop${wizard ? ' wizard-backdrop' : ''}`} role="presentation" onClick={onClose}>
      <div
        className={`modal-card${wide ? ' modal-wide' : ''}${wizard ? ' modal-wizard' : ''}`}
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <h2>{title}</h2>
          <button type="button" className="secondary modal-close" onClick={onClose} aria-label="Close">
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
