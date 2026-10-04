import { useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

export function Dialog({ children, className, label, labelledBy, busy = false, onClose }: {
  children: ReactNode;
  className: string;
  label?: string;
  labelledBy?: string;
  busy?: boolean;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const opener = useRef(document.activeElement);
  const dismiss = useRef({ busy, onClose });
  dismiss.current = { busy, onClose };
  const [native] = useState(() => typeof HTMLDialogElement !== 'undefined' && typeof HTMLDialogElement.prototype.showModal === 'function');
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const root = document.getElementById('root');
    const previousHidden = root?.getAttribute('aria-hidden');
    const focusable = () => [...dialog.querySelectorAll<HTMLElement>('button, input, select, a[href], summary, [tabindex]')]
      .filter((el) => !el.matches(':disabled') && el.tabIndex >= 0 && el.getClientRects().length > 0);
    const focusFirst = () => (dialog.querySelector<HTMLElement>('[data-autofocus]:not(:disabled), [autofocus]:not(:disabled)') ?? focusable()[0] ?? dialog).focus();
    const trapFocus = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        if (!dismiss.current.busy) dismiss.current.onClose();
      } else if (event.key === 'Tab') {
        const targets = focusable();
        const first = targets[0] ?? dialog;
        const last = targets[targets.length - 1] ?? dialog;
        if ((event.shiftKey && document.activeElement === first) || (!event.shiftKey && document.activeElement === last) || !dialog.contains(document.activeElement)) {
          event.preventDefault();
          (event.shiftKey ? last : first).focus();
        }
      }
    };
    const containFocus = (event: FocusEvent) => { if (!dialog.contains(event.target as Node)) focusFirst(); };
    if (native) dialog.showModal();
    else {
      // macOS 12's older system WebKit predates showModal; retain a working modal there.
      dialog.setAttribute('open', '');
      root?.setAttribute('aria-hidden', 'true');
      document.addEventListener('keydown', trapFocus);
      document.addEventListener('focusin', containFocus);
    }
    focusFirst();
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      if (native) dialog.close();
      else {
        document.removeEventListener('keydown', trapFocus);
        document.removeEventListener('focusin', containFocus);
        if (previousHidden == null) root?.removeAttribute('aria-hidden');
        else root?.setAttribute('aria-hidden', previousHidden);
      }
      document.body.style.overflow = previousOverflow;
      const previousFocus = opener.current;
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
    };
  }, [native]);

  const content = <dialog ref={ref} className={className} role="dialog" tabIndex={-1} aria-label={label} aria-labelledby={labelledBy} aria-modal="true" aria-busy={busy || undefined}
      onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }}
      onClick={(event) => {
        // Native dialog backdrops target the dialog; clicks in its padding must stay open.
        const rect = event.currentTarget.getBoundingClientRect();
        if (event.target === event.currentTarget && !busy &&
          (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom)) onClose();
      }}>
      {children}
    </dialog>;
  return createPortal(native ? content : <div className={`dialog-backdrop${className === 'detail-drawer' ? ' drawer-backdrop' : ''}`} onClick={(event) => {
    if (event.target === event.currentTarget && !busy) onClose();
  }}>{content}</div>, document.body);
}
