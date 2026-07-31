import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  type PropsWithChildren,
} from "react";
import type { IBufferCell, ITheme, Terminal } from "@xterm/xterm";

type Disposable = { dispose: () => void };

type Registration = {
  id: string;
  term: Terminal;
  host: HTMLElement;
  raster: HTMLCanvasElement;
  texture: WebGLTexture | null;
  disposables: Disposable[];
};

export type AgentsSharedTerminalSurface = {
  register: (id: string, term: Terminal, host: HTMLElement) => Disposable;
  invalidateLayout: () => void;
};

const SurfaceContext = createContext<AgentsSharedTerminalSurface | null>(null);

export function useAgentsSharedTerminalSurface() {
  return useContext(SurfaceContext);
}

const VERTEX_SHADER = `#version 300 es
in vec2 a_position;
uniform vec2 u_canvas;
uniform vec4 u_rect;
out vec2 v_texcoord;
void main() {
  vec2 pixel = u_rect.xy + a_position * u_rect.zw;
  vec2 clip = vec2(pixel.x / u_canvas.x * 2.0 - 1.0, 1.0 - pixel.y / u_canvas.y * 2.0);
  gl_Position = vec4(clip, 0.0, 1.0);
  v_texcoord = a_position;
}`;

const FRAGMENT_SHADER = `#version 300 es
precision mediump float;
uniform sampler2D u_texture;
in vec2 v_texcoord;
out vec4 out_color;
void main() {
  out_color = texture(u_texture, v_texcoord);
}`;

const ANSI_THEME_KEYS = [
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue",
  "brightMagenta", "brightCyan", "brightWhite",
] as const;

// xterm's resolved palette uses these values whenever a theme does not
// override one of the first 16 ANSI colors. term.options.theme only exposes
// the caller's overrides, so the compositor needs the same fallbacks.
const DEFAULT_ANSI_COLORS = [
  "#2e3436", "#cc0000", "#4e9a06", "#c4a000", "#3465a4", "#75507b", "#06989a", "#d3d7cf",
  "#555753", "#ef2929", "#8ae234", "#fce94f", "#729fcf", "#ad7fa8", "#34e2e2", "#eeeeec",
] as const;

function compileShader(gl: WebGL2RenderingContext, type: number, source: string) {
  const shader = gl.createShader(type);
  if (!shader) throw new Error("Unable to create shared terminal shader");
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const message = gl.getShaderInfoLog(shader) || "Unknown shader compile error";
    gl.deleteShader(shader);
    throw new Error(message);
  }
  return shader;
}

function createProgram(gl: WebGL2RenderingContext) {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER);
  const fragment = compileShader(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER);
  const program = gl.createProgram();
  if (!program) throw new Error("Unable to create shared terminal program");
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  gl.deleteShader(vertex);
  gl.deleteShader(fragment);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const message = gl.getProgramInfoLog(program) || "Unknown shader link error";
    gl.deleteProgram(program);
    throw new Error(message);
  }
  return program;
}

function cssColor(value: number) {
  return `#${value.toString(16).padStart(6, "0")}`;
}

function paletteColor(index: number, theme: ITheme, foreground: string) {
  if (index < 16) return theme[ANSI_THEME_KEYS[index]] ?? DEFAULT_ANSI_COLORS[index] ?? foreground;
  if (index < 232) {
    const value = index - 16;
    const channel = (part: number) => part === 0 ? 0 : 55 + part * 40;
    const red = channel(Math.floor(value / 36));
    const green = channel(Math.floor(value / 6) % 6);
    const blue = channel(value % 6);
    return cssColor((red << 16) | (green << 8) | blue);
  }
  const gray = 8 + (index - 232) * 10;
  return cssColor((gray << 16) | (gray << 8) | gray);
}

type Rgb = { red: number; green: number; blue: number };
const contrastColorCache = new Map<string, string>();

function parseHexColor(value: string): Rgb | null {
  const match = /^#([\da-f]{3}|[\da-f]{6})$/i.exec(value.trim());
  if (!match) return null;
  const hex = match[1].length === 3
    ? match[1].split("").map((channel) => `${channel}${channel}`).join("")
    : match[1];
  return {
    red: Number.parseInt(hex.slice(0, 2), 16),
    green: Number.parseInt(hex.slice(2, 4), 16),
    blue: Number.parseInt(hex.slice(4, 6), 16),
  };
}

function relativeLuminance(color: Rgb) {
  const channel = (value: number) => {
    const normalized = value / 255;
    return normalized <= 0.03928
      ? normalized / 12.92
      : ((normalized + 0.055) / 1.055) ** 2.4;
  };
  return channel(color.red) * 0.2126 + channel(color.green) * 0.7152 + channel(color.blue) * 0.0722;
}

function contrastRatio(first: Rgb, second: Rgb) {
  const firstLuminance = relativeLuminance(first);
  const secondLuminance = relativeLuminance(second);
  return (Math.max(firstLuminance, secondLuminance) + 0.05)
    / (Math.min(firstLuminance, secondLuminance) + 0.05);
}

function mixColor(from: Rgb, to: Rgb, amount: number): Rgb {
  return {
    red: Math.round(from.red + (to.red - from.red) * amount),
    green: Math.round(from.green + (to.green - from.green) * amount),
    blue: Math.round(from.blue + (to.blue - from.blue) * amount),
  };
}

function rgbColor(color: Rgb) {
  return `#${[color.red, color.green, color.blue]
    .map((channel) => channel.toString(16).padStart(2, "0"))
    .join("")}`;
}

/** Match xterm's minimum-contrast behavior for compositor-rendered glyphs. */
export function adjustTerminalForegroundContrast(
  foreground: string,
  background: string,
  minimumRatio: number,
) {
  const cacheKey = `${foreground}\0${background}\0${minimumRatio}`;
  const cached = contrastColorCache.get(cacheKey);
  if (cached) return cached;
  const foregroundRgb = parseHexColor(foreground);
  const backgroundRgb = parseHexColor(background);
  if (!foregroundRgb || !backgroundRgb || minimumRatio <= 1
    || contrastRatio(foregroundRgb, backgroundRgb) >= minimumRatio) {
    contrastColorCache.set(cacheKey, foreground);
    return foreground;
  }

  const candidates = [
    { red: 0, green: 0, blue: 0 },
    { red: 255, green: 255, blue: 255 },
  ];
  let best: { color: Rgb; amount: number; ratio: number } | null = null;
  for (const target of candidates) {
    let low = 0;
    let high = 1;
    for (let step = 0; step < 10; step++) {
      const amount = (low + high) / 2;
      if (contrastRatio(mixColor(foregroundRgb, target, amount), backgroundRgb) >= minimumRatio) {
        high = amount;
      } else {
        low = amount;
      }
    }
    const color = mixColor(foregroundRgb, target, high);
    const ratio = contrastRatio(color, backgroundRgb);
    if (ratio >= minimumRatio && (!best || high < best.amount)) {
      best = { color, amount: high, ratio };
    } else if (!best || ratio > best.ratio) {
      best = { color, amount: high, ratio };
    }
  }
  const adjusted = best ? rgbColor(best.color) : foreground;
  contrastColorCache.set(cacheKey, adjusted);
  return adjusted;
}

function cellColor(cell: IBufferCell, foreground: boolean, theme: ITheme) {
  const useForeground = cell.isInverse() ? !foreground : foreground;
  if (useForeground ? cell.isFgRGB() : cell.isBgRGB()) {
    return cssColor(useForeground ? cell.getFgColor() : cell.getBgColor());
  }
  if (useForeground ? cell.isFgPalette() : cell.isBgPalette()) {
    return paletteColor(useForeground ? cell.getFgColor() : cell.getBgColor(), theme, theme.foreground ?? "#ffffff");
  }
  return useForeground ? (theme.foreground ?? "#ffffff") : (theme.background ?? "#000000");
}

function selectionContains(term: Terminal, x: number, absoluteY: number) {
  const range = term.getSelectionPosition();
  if (!range) return false;
  const startY = range.start.y - 1;
  const endY = range.end.y - 1;
  if (absoluteY < startY || absoluteY > endY) return false;
  const startX = range.start.x - 1;
  const endX = range.end.x - 1;
  if (startY === endY) return x >= startX && x < endX;
  if (absoluteY === startY) return x >= startX;
  if (absoluteY === endY) return x < endX;
  return true;
}

function drawBlockGlyph(
  context: CanvasRenderingContext2D,
  chars: string,
  x: number,
  y: number,
  width: number,
  height: number,
) {
  const blocks: Record<string, [number, number, number, number]> = {
    "▀": [0, 0, 1, 0.5], "▄": [0, 0.5, 1, 0.5], "█": [0, 0, 1, 1],
    "▌": [0, 0, 0.5, 1], "▐": [0.5, 0, 0.5, 1], "▖": [0, 0.5, 0.5, 0.5],
    "▗": [0.5, 0.5, 0.5, 0.5], "▘": [0, 0, 0.5, 0.5], "▝": [0.5, 0, 0.5, 0.5],
  };
  const block = blocks[chars];
  if (!block) return false;
  context.fillRect(x + block[0] * width, y + block[1] * height, block[2] * width, block[3] * height);
  return true;
}

/** Rasterize one public xterm viewport without reaching into xterm internals. */
function rasterize(registration: Registration, width: number, height: number, dpr: number) {
  const { term, raster } = registration;
  const pixelWidth = Math.max(1, Math.round(width * dpr));
  const pixelHeight = Math.max(1, Math.round(height * dpr));
  if (raster.width !== pixelWidth) raster.width = pixelWidth;
  if (raster.height !== pixelHeight) raster.height = pixelHeight;
  const context = raster.getContext("2d", { alpha: true });
  if (!context) return;

  const theme = term.options.theme ?? {};
  const background = theme.background ?? "#000000";
  const foreground = theme.foreground ?? "#ffffff";
  const cellWidth = pixelWidth / Math.max(1, term.cols);
  const cellHeight = pixelHeight / Math.max(1, term.rows);
  const fontSize = Number(term.options.fontSize ?? 14) * dpr;
  const family = String(term.options.fontFamily ?? "monospace");
  const normalFontWeight = term.options.fontWeight ?? "normal";
  const boldFontWeight = term.options.fontWeightBold ?? "bold";
  context.clearRect(0, 0, pixelWidth, pixelHeight);
  context.fillStyle = background;
  context.fillRect(0, 0, pixelWidth, pixelHeight);
  context.textBaseline = "middle";
  context.textAlign = "left";

  const buffer = term.buffer.active;
  const workCell = buffer.getNullCell();
  for (let row = 0; row < term.rows; row++) {
    const absoluteY = buffer.viewportY + row;
    const line = buffer.getLine(absoluteY);
    if (!line) continue;
    for (let column = 0; column < term.cols; column++) {
      const cell = line.getCell(column, workCell);
      if (!cell || cell.getWidth() === 0) continue;
      const x = column * cellWidth;
      const y = row * cellHeight;
      const selected = selectionContains(term, column, absoluteY);
      const cellBackground = selected ? (theme.selectionBackground ?? "#555555") : cellColor(cell, false, theme);
      context.fillStyle = cellBackground;
      context.fillRect(x, y, cellWidth * Math.max(1, cell.getWidth()), cellHeight);
      const chars = cell.getChars();
      if (!chars || cell.isInvisible()) continue;
      context.globalAlpha = cell.isDim() ? 0.55 : 1;
      const cellForeground = selected && theme.selectionForeground
        ? theme.selectionForeground
        : cellColor(cell, true, theme);
      const minimumContrastRatio = Math.max(
        1,
        Number(term.options.minimumContrastRatio ?? 1) / (cell.isDim() ? 2 : 1),
      );
      context.fillStyle = adjustTerminalForegroundContrast(
        cellForeground,
        cellBackground,
        minimumContrastRatio,
      );
      context.font = `${cell.isItalic() ? "italic " : ""}${cell.isBold() ? boldFontWeight : normalFontWeight} ${fontSize}px ${family}`;
      if (!drawBlockGlyph(context, chars, x, y, cellWidth * Math.max(1, cell.getWidth()), cellHeight)) {
        context.fillText(chars, x, y + cellHeight / 2, cellWidth * Math.max(1, cell.getWidth()));
      }
      context.globalAlpha = 1;
      const decorationY = y + cellHeight - Math.max(1, dpr);
      if (cell.isUnderline()) context.fillRect(x, decorationY, cellWidth * Math.max(1, cell.getWidth()), Math.max(1, dpr));
      if (cell.isStrikethrough()) context.fillRect(x, y + cellHeight / 2, cellWidth * Math.max(1, cell.getWidth()), Math.max(1, dpr));
      if (cell.isOverline()) context.fillRect(x, y, cellWidth * Math.max(1, cell.getWidth()), Math.max(1, dpr));
    }
  }

  const focused = term.textarea === term.element?.ownerDocument.activeElement;
  const cursorRow = buffer.baseY + buffer.cursorY - buffer.viewportY;
  if (focused && cursorRow >= 0 && cursorRow < term.rows && buffer.cursorX < term.cols) {
    const cursorX = buffer.cursorX * cellWidth;
    const cursorY = cursorRow * cellHeight;
    const cursorStyle = term.options.cursorStyle ?? "block";
    context.fillStyle = theme.cursor ?? foreground;
    if (cursorStyle === "bar") {
      context.fillRect(cursorX, cursorY, Math.max(1, Number(term.options.cursorWidth ?? 1) * dpr), cellHeight);
    } else if (cursorStyle === "underline") {
      context.fillRect(cursorX, cursorY + cellHeight - Math.max(1, dpr), cellWidth, Math.max(1, dpr));
    } else {
      const cursorCell = buffer.getLine(buffer.baseY + buffer.cursorY)?.getCell(buffer.cursorX, workCell);
      const cursorWidth = cellWidth * Math.max(1, cursorCell?.getWidth() ?? 1);
      context.fillRect(cursorX, cursorY, cursorWidth, cellHeight);
      const chars = cursorCell?.getChars();
      if (cursorCell && chars && !cursorCell.isInvisible()) {
        context.fillStyle = theme.cursorAccent ?? background;
        context.font = `${cursorCell.isItalic() ? "italic " : ""}${cursorCell.isBold() ? boldFontWeight : normalFontWeight} ${fontSize}px ${family}`;
        if (!drawBlockGlyph(context, chars, cursorX, cursorY, cursorWidth, cellHeight)) {
          context.fillText(chars, cursorX, cursorY + cellHeight / 2, cursorWidth);
        }
      }
    }
  }
}

/** Owns the sole GPU context for one Agents Overview surface. */
class SharedSurfaceController implements AgentsSharedTerminalSurface {
  #canvas: HTMLCanvasElement | null = null;
  #root: HTMLElement | null = null;
  #gl: WebGL2RenderingContext | null = null;
  #program: WebGLProgram | null = null;
  #buffer: WebGLBuffer | null = null;
  #registrations = new Map<string, Registration>();
  #interactionFallbacks = new Set<string>();
  #frame: number | null = null;
  #failed = false;

  attach(canvas: HTMLCanvasElement, root: HTMLElement) {
    this.#canvas = canvas;
    this.#root = root;
    canvas.addEventListener("webglcontextlost", this.#handleContextLost);
    canvas.addEventListener("webglcontextrestored", this.#handleContextRestored);
    root.addEventListener("scroll", this.invalidateLayout, true);
    window.addEventListener("resize", this.invalidateLayout);
    window.addEventListener("keyup", this.#handleModifierRelease);
    window.addEventListener("blur", this.#clearInteractionFallbacks);
    this.#initialize();
    this.invalidateLayout();
    return () => this.dispose();
  }

  register(id: string, term: Terminal, host: HTMLElement): Disposable {
    this.#remove(id);
    const previousReflowCursorLine = term.options.reflowCursorLine;
    // Grid and maximize transitions resize a presentation without necessarily
    // prompting the provider to repaint its current line. Preserve that line
    // across local width changes; the standalone path keeps xterm's default.
    term.options.reflowCursorLine = true;
    const registration: Registration = {
      id,
      term,
      host,
      raster: document.createElement("canvas"),
      texture: null,
      disposables: [],
    };
    const invalidate = () => this.invalidateLayout();
    const useDomInteractionLayer = (event: PointerEvent) => {
      this.#setInteractionFallback(id, event.ctrlKey || event.metaKey);
    };
    const restoreSharedLayer = () => this.#setInteractionFallback(id, false);
    host.addEventListener("pointermove", useDomInteractionLayer);
    host.addEventListener("pointerleave", restoreSharedLayer);
    const subscribe = (event: ((listener: () => void) => Disposable) | undefined) => {
      const disposable = typeof event === "function" ? event(invalidate) : undefined;
      return disposable && typeof disposable.dispose === "function" ? disposable : null;
    };
    registration.disposables = [
      subscribe(term.onRender), subscribe(term.onWriteParsed), subscribe(term.onScroll),
      subscribe(term.onResize), subscribe(term.onSelectionChange), subscribe(term.buffer.onBufferChange),
      {
        dispose: () => {
          term.options.reflowCursorLine = previousReflowCursorLine;
          host.removeEventListener("pointermove", useDomInteractionLayer);
          host.removeEventListener("pointerleave", restoreSharedLayer);
          restoreSharedLayer();
        },
      },
    ].filter((value): value is Disposable => value !== null);
    this.#registrations.set(id, registration);
    this.invalidateLayout();
    return { dispose: () => this.#remove(id) };
  }

  invalidateLayout = () => {
    if (this.#frame !== null || this.#failed) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = null;
      this.#render();
    });
  };

  #initialize() {
    if (!this.#canvas) return;
    if (typeof WebGL2RenderingContext === "undefined") {
      this.#failed = true;
      this.#canvas.style.display = "none";
      return;
    }
    const gl = this.#canvas.getContext("webgl2", { alpha: true, antialias: false });
    if (!gl) {
      this.#failed = true;
      this.#canvas.style.display = "none";
      return;
    }
    try {
      this.#gl = gl;
      this.#program = createProgram(gl);
      this.#buffer = gl.createBuffer();
      if (!this.#buffer) throw new Error("Unable to create shared terminal vertex buffer");
      gl.bindBuffer(gl.ARRAY_BUFFER, this.#buffer);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1]), gl.STATIC_DRAW);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      this.#canvas.style.display = "";
    } catch (error) {
      console.warn("Agents shared terminal compositor unavailable; using xterm DOM renderers.", error);
      this.#failed = true;
      this.#canvas.style.display = "none";
      this.#gl = null;
      this.#program = null;
      this.#buffer = null;
    }
  }

  #render() {
    const canvas = this.#canvas;
    const root = this.#root;
    const gl = this.#gl;
    const program = this.#program;
    if (!canvas || !root || !gl || !program || this.#failed) return;
    const rootRect = root.getBoundingClientRect();
    if (rootRect.width < 2 || rootRect.height < 2) return;
    const dpr = window.devicePixelRatio || 1;
    const width = Math.max(1, Math.round(rootRect.width * dpr));
    const height = Math.max(1, Math.round(rootRect.height * dpr));
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    gl.viewport(0, 0, width, height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.useProgram(program);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.#buffer);
    const position = gl.getAttribLocation(program, "a_position");
    gl.enableVertexAttribArray(position);
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
    gl.uniform2f(gl.getUniformLocation(program, "u_canvas"), width, height);
    gl.uniform1i(gl.getUniformLocation(program, "u_texture"), 0);

    for (const registration of this.#registrations.values()) {
      if (!registration.host.isConnected) continue;
      const screen = registration.term.element?.querySelector<HTMLElement>(".xterm-screen");
      const viewport = registration.term.element?.querySelector<HTMLElement>(".xterm-viewport");
      const rect = (screen ?? registration.host).getBoundingClientRect();
      const viewportRect = viewport?.getBoundingClientRect();
      const viewportContentRight = viewport && viewportRect && viewport.clientWidth > 0
        ? viewportRect.left + viewport.clientWidth
        : rect.right;
      const left = Math.max(rect.left, rootRect.left);
      const top = Math.max(rect.top, rootRect.top);
      // xterm's screen can extend beneath its native scrollbar. Keep the quad
      // at full cell width, but clip before the viewport gutter so the real
      // scrollbar remains completely visible and interactive.
      const right = Math.min(rect.right, rootRect.right, viewportContentRight);
      const bottom = Math.min(rect.bottom, rootRect.bottom);
      if (right <= left || bottom <= top || rect.width < 2 || rect.height < 2) continue;
      rasterize(registration, rect.width, rect.height, dpr);
      registration.texture ??= gl.createTexture();
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, registration.texture);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, registration.raster);
      // The tile is already rasterized at device resolution. Align its quad to
      // the same integer pixel grid so the GPU does not resample glyph edges.
      const x = Math.round((rect.left - rootRect.left) * dpr);
      const y = Math.round((rect.top - rootRect.top) * dpr);
      gl.uniform4f(
        gl.getUniformLocation(program, "u_rect"),
        x,
        y,
        registration.raster.width,
        registration.raster.height,
      );
      gl.enable(gl.SCISSOR_TEST);
      gl.scissor(
        Math.round((left - rootRect.left) * dpr),
        Math.round((rootRect.bottom - bottom) * dpr),
        Math.round((right - left) * dpr),
        Math.round((bottom - top) * dpr),
      );
      gl.drawArrays(gl.TRIANGLES, 0, 6);
    }
    gl.disable(gl.SCISSOR_TEST);
  }

  #remove(id: string) {
    const registration = this.#registrations.get(id);
    if (!registration) return;
    registration.disposables.forEach((disposable) => disposable.dispose());
    if (registration.texture && this.#gl) this.#gl.deleteTexture(registration.texture);
    this.#registrations.delete(id);
    this.invalidateLayout();
  }

  #setInteractionFallback(id: string, enabled: boolean) {
    if (!this.#canvas) return;
    if (enabled) this.#interactionFallbacks.add(id);
    else this.#interactionFallbacks.delete(id);
    this.#canvas.style.opacity = this.#interactionFallbacks.size > 0 ? "0" : "1";
    if (this.#interactionFallbacks.size === 0) this.invalidateLayout();
  }

  #clearInteractionFallbacks = () => {
    this.#interactionFallbacks.clear();
    if (this.#canvas) this.#canvas.style.opacity = "1";
    this.invalidateLayout();
  };

  #handleModifierRelease = (event: KeyboardEvent) => {
    if (event.key === "Control" || event.key === "Meta") this.#clearInteractionFallbacks();
  };

  #handleContextLost = (event: Event) => {
    event.preventDefault();
    this.#failed = true;
    if (this.#canvas) this.#canvas.style.visibility = "hidden";
  };

  #handleContextRestored = () => {
    this.#failed = false;
    for (const registration of this.#registrations.values()) registration.texture = null;
    this.#initialize();
    if (this.#canvas) this.#canvas.style.visibility = "visible";
    this.invalidateLayout();
  };

  dispose() {
    if (this.#frame !== null) cancelAnimationFrame(this.#frame);
    window.removeEventListener("resize", this.invalidateLayout);
    window.removeEventListener("keyup", this.#handleModifierRelease);
    window.removeEventListener("blur", this.#clearInteractionFallbacks);
    this.#root?.removeEventListener("scroll", this.invalidateLayout, true);
    for (const id of Array.from(this.#registrations.keys())) this.#remove(id);
    if (this.#buffer && this.#gl) this.#gl.deleteBuffer(this.#buffer);
    if (this.#program && this.#gl) this.#gl.deleteProgram(this.#program);
    this.#canvas?.removeEventListener("webglcontextlost", this.#handleContextLost);
    this.#canvas?.removeEventListener("webglcontextrestored", this.#handleContextRestored);
    this.#gl?.getExtension("WEBGL_lose_context")?.loseContext();
    this.#canvas = null;
    this.#root = null;
    this.#gl = null;
    this.#interactionFallbacks.clear();
  }
}

export function AgentsSharedTerminalSurfaceProvider({ children }: PropsWithChildren) {
  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const controller = useMemo(() => new SharedSurfaceController(), []);
  useEffect(() => {
    if (!rootRef.current || !canvasRef.current) return;
    return controller.attach(canvasRef.current, rootRef.current);
  }, [controller]);
  useLayoutEffect(() => {
    controller.invalidateLayout();
  });
  return (
    <SurfaceContext.Provider value={controller}>
      <div ref={rootRef} className="agents-shared-terminal-surface relative h-full min-h-0 min-w-0 overflow-hidden" data-testid="agents-shared-terminal-surface">
        {children}
        <canvas
          ref={canvasRef}
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 z-20 h-full w-full"
          data-testid="agents-shared-terminal-canvas"
        />
      </div>
    </SurfaceContext.Provider>
  );
}
