import { describe, expect, it } from "vitest";
import {
  DEFAULT_AUTO_SCROLL_SPEED,
  computeAutoScrollSpeed,
} from "./dragAutoScroll";

const bounds = { top: 100, height: 400 };

describe("computeAutoScrollSpeed", () => {
  it("stays still while the pointer is in the middle of the list", () => {
    expect(computeAutoScrollSpeed(300, bounds)).toBe(0);
  });

  it("scrolls up as the pointer approaches the top edge", () => {
    const speed = computeAutoScrollSpeed(110, bounds);
    expect(speed).toBeLessThan(0);
    expect(speed).toBeGreaterThan(-DEFAULT_AUTO_SCROLL_SPEED);
  });

  it("scrolls down as the pointer approaches the bottom edge", () => {
    const speed = computeAutoScrollSpeed(490, bounds);
    expect(speed).toBeGreaterThan(0);
    expect(speed).toBeLessThan(DEFAULT_AUTO_SCROLL_SPEED);
  });

  it("runs at full speed once the pointer passes the container edge", () => {
    expect(computeAutoScrollSpeed(20, bounds)).toBe(-DEFAULT_AUTO_SCROLL_SPEED);
    expect(computeAutoScrollSpeed(900, bounds)).toBe(DEFAULT_AUTO_SCROLL_SPEED);
  });

  it("accelerates monotonically toward the edge", () => {
    const near = Math.abs(computeAutoScrollSpeed(101, bounds));
    const mid = Math.abs(computeAutoScrollSpeed(120, bounds));
    const far = Math.abs(computeAutoScrollSpeed(150, bounds));
    expect(near).toBeGreaterThan(mid);
    expect(mid).toBeGreaterThan(far);
  });

  it("honours custom edge and speed settings", () => {
    const speed = computeAutoScrollSpeed(100, bounds, { edgeSize: 20, maxSpeed: 200 });
    expect(speed).toBe(-200);
    expect(computeAutoScrollSpeed(130, bounds, { edgeSize: 20, maxSpeed: 200 })).toBe(0);
  });

  it("never widens the hot zones past half the container", () => {
    // A 20px list would otherwise be entirely hot zone with no neutral band.
    const shortList = { top: 0, height: 20 };
    expect(computeAutoScrollSpeed(10, shortList)).toBe(0);
    expect(computeAutoScrollSpeed(0, shortList)).toBe(-DEFAULT_AUTO_SCROLL_SPEED);
  });

  it("ignores degenerate containers and pointer positions", () => {
    expect(computeAutoScrollSpeed(50, { top: 0, height: 0 })).toBe(0);
    expect(computeAutoScrollSpeed(Number.NaN, bounds)).toBe(0);
  });
});
