import { describe, expect, it } from 'vitest';
import { formatLatency, formatTps } from './format';

describe('formatTps', () => {
  it('distinguishes missing throughput from zero and retains the token/s unit', () => {
    expect(formatTps(null)).toBe('—');
    expect(formatTps(undefined)).toBe('—');
    expect(formatTps(0)).toBe('0.0 token/s');
    expect(formatTps(12.34)).toBe('12.3 token/s');
    expect(formatTps(12.36)).toBe('12.4 token/s');
  });
});

describe('formatLatency', () => {
  it('distinguishes missing timing from zero and preserves seconds for long delays', () => {
    expect(formatLatency(null)).toBe('—');
    expect(formatLatency(undefined)).toBe('—');
    expect(formatLatency(0)).toBe('0 ms');
    expect(formatLatency(123.6)).toBe('124 ms');
    expect(formatLatency(999)).toBe('999 ms');
    expect(formatLatency(1_000)).toBe('1.00 s');
    expect(formatLatency(1_234.5)).toBe('1.23 s');
    expect(formatLatency(90_125)).toBe('90.13 s');
  });
});
