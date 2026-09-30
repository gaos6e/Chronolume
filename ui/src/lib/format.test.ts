import { describe, expect, it } from 'vitest';
import { formatTps } from './format';

describe('formatTps', () => {
  it('distinguishes missing throughput from zero and retains the token/s unit', () => {
    expect(formatTps(null)).toBe('—');
    expect(formatTps(undefined)).toBe('—');
    expect(formatTps(0)).toBe('0.0 token/s');
    expect(formatTps(12.34)).toBe('12.3 token/s');
    expect(formatTps(12.36)).toBe('12.4 token/s');
  });
});
