// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import { formatDuration, formatBytes, formatClock } from "./format.mjs";

test("formatDuration table", () => {
  /** @type {Array<[number, string]>} */
  const cases = [
    [0, "0ms"],
    [1, "1ms"],
    [850, "850ms"],
    [999, "999ms"],
    [1000, "1s"],
    [1500, "1s"],
    [59_000, "59s"],
    [60_000, "1m"],
    [61_000, "1m 1s"],
    [123_000, "2m 3s"],
    [590_000, "9m 50s"],
    [3_600_000, "1h"],
    [3_660_000, "1h 1m"],
    [3_720_000, "1h 2m"],
    [3_723_000, "1h 2m 3s"],
    [7_200_000, "2h"],
    [-5, "0ms"],
    [Number.NaN, "0ms"],
  ];
  for (const [input, expected] of cases) {
    assert.equal(formatDuration(input), expected, `formatDuration(${input})`);
  }
});

test("formatBytes table", () => {
  /** @type {Array<[number, string]>} */
  const cases = [
    [0, "0B"],
    [123, "123B"],
    [1023, "1023B"],
    [1024, "1.0KB"],
    [1536, "1.5KB"],
    [4608, "4.5KB"],
    [10 * 1024, "10KB"],
    [999 * 1024, "999KB"],
    [1024 * 1024, "1.0MB"],
    [1_258_291, "1.2MB"],
    [15 * 1024 * 1024, "15MB"],
    [3 * 1024 ** 3, "3.0GB"],
    [-1, "0B"],
    [Number.NaN, "0B"],
  ];
  for (const [input, expected] of cases) {
    assert.equal(formatBytes(input), expected, `formatBytes(${input})`);
  }
});

test("formatClock renders a local 24-hour hh:mm:ss wall clock", () => {
  // 12:34:56 local time on 2026-01-02 (timezone-agnostic: derive the epoch
  // from the same local fields the formatter reads back).
  const ms = new Date(2026, 0, 2, 12, 34, 56).getTime();
  assert.equal(formatClock(ms), "12:34:56", "midday clock");
  const early = new Date(2026, 0, 2, 3, 5, 7).getTime();
  assert.equal(formatClock(early), "03:05:07", "zero-padded clock");
});

test("formatClock rejects non-finite timestamps", () => {
  assert.equal(formatClock(Number.NaN), "--:--:--");
});
