import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Dialog } from './Dialog';

describe('Dialog compatibility', () => {
  it('supports Escape and restores focus in WebViews without showModal', () => {
    const original = HTMLDialogElement.prototype.showModal;
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { value: undefined, configurable: true, writable: true });
    const opener = document.createElement('button');
    document.body.append(opener);
    opener.focus();
    try {
      const close = vi.fn();
      const { unmount } = render(<Dialog className="editor-dialog" label="Editor" onClose={close}><input aria-label="Alias" autoFocus data-autofocus /><button type="button">Save</button></Dialog>);
      expect(screen.getByRole('dialog', { name: 'Editor' })).toHaveAttribute('open');
      expect(screen.getByLabelText('Alias')).toHaveFocus();
      fireEvent.keyDown(document, { key: 'Escape' });
      expect(close).toHaveBeenCalledOnce();
      unmount();
      expect(opener).toHaveFocus();
    } finally {
      HTMLDialogElement.prototype.showModal = original;
      opener.remove();
    }
  });
});
