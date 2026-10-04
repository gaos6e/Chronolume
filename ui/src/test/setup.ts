import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// jsdom does not implement the native dialog lifecycle. Browser checks cover focus trapping.
if (!HTMLDialogElement.prototype.showModal) {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute('open', '');
    this.querySelector<HTMLElement>('[autofocus], button, input')?.focus();
  };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
}

afterEach(cleanup);
