#!/usr/bin/env node

const endpoint = process.argv.slice(2).find((argument) => !argument.startsWith("--"))
  ?? "http://127.0.0.1:9222";
const collectGarbage = process.argv.includes("--collect-garbage");
const sampleArgument = process.argv.find((argument) => argument.startsWith("--sample-seconds="));
const sampleSeconds = sampleArgument
  ? Math.max(0, Number.parseInt(sampleArgument.split("=")[1] ?? "0", 10) || 0)
  : 0;
const targets = await fetch(`${endpoint}/json`).then((response) => {
  if (!response.ok) throw new Error(`DevTools target listing failed: ${response.status}`);
  return response.json();
});
const target = targets.find((candidate) => candidate.type === "page" && candidate.webSocketDebuggerUrl);
if (!target) throw new Error("No Wardian WebView page target found");
if (typeof WebSocket === "undefined") {
  throw new Error("This profiler requires a Node runtime with global WebSocket support");
}

const socket = new WebSocket(target.webSocketDebuggerUrl);
const pending = new Map();
let nextId = 1;

socket.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
  if (!message.id) return;
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error(message.error.message));
  else waiter.resolve(message.result);
});

await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", () => reject(new Error("DevTools WebSocket failed")), {
    once: true,
  });
});

function command(method, params = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

const mib = (bytes) => Math.round((bytes / (1024 * 1024)) * 10) / 10;
await command("Performance.enable");
if (collectGarbage) await command("HeapProfiler.collectGarbage");
let allocationSample = [];
if (sampleSeconds > 0) {
  await command("HeapProfiler.startSampling", { samplingInterval: 32 * 1024 });
  await new Promise((resolve) => setTimeout(resolve, sampleSeconds * 1_000));
  const { profile } = await command("HeapProfiler.stopSampling");
  const allocations = new Map();
  const visit = (node) => {
    if (node.selfSize > 0) {
      const frame = node.callFrame;
      const script = frame.url.split("/").pop() || "inline";
      const key = `${frame.functionName || "(anonymous)"} (${script}:${frame.lineNumber + 1})`;
      allocations.set(key, (allocations.get(key) ?? 0) + node.selfSize);
    }
    for (const child of node.children ?? []) visit(child);
  };
  visit(profile.head);
  allocationSample = [...allocations.entries()]
    .map(([frame, bytes]) => ({ frame, allocated_mib: mib(bytes) }))
    .sort((left, right) => right.allocated_mib - left.allocated_mib)
    .slice(0, 15);
}
const [performance, dom, heap, page] = await Promise.all([
  command("Performance.getMetrics"),
  command("Memory.getDOMCounters"),
  command("Runtime.getHeapUsage"),
  command("Runtime.evaluate", {
    expression: `(() => ({
      title: document.title,
      protocol: location.protocol,
      elements: document.getElementsByTagName("*").length,
      xterm_hosts: document.querySelectorAll(".xterm").length,
      xterm_canvases: document.querySelectorAll(".xterm canvas").length,
      canvases: document.querySelectorAll("canvas").length,
      surfaces: Array.from(document.querySelectorAll("[data-surface-type]"), surface => {
        const rect = surface.getBoundingClientRect();
        const style = getComputedStyle(surface);
        return {
          type: surface.getAttribute("data-surface-type"),
          visible: style.display !== "none" && style.visibility !== "hidden"
            && rect.width > 0 && rect.height > 0,
          width: Math.round(rect.width),
          height: Math.round(rect.height)
        };
      }),
      canvas_details: Array.from(document.querySelectorAll("canvas"), canvas => ({
        class_name: typeof canvas.className === "string" ? canvas.className : "",
        width: canvas.width,
        height: canvas.height,
        surface_type: canvas.closest("[data-surface-type]")?.getAttribute("data-surface-type") ?? null,
        hidden: canvas.closest('[aria-hidden="true"], [hidden]') !== null
      })),
      iframes: document.querySelectorAll("iframe").length,
      body_text_chars: document.body?.innerText.length ?? 0
    }))()`,
    returnByValue: true,
  }),
]);

const metrics = Object.fromEntries(
  performance.metrics.map((metric) => [metric.name, metric.value]),
);
const output = {
  schema: 1,
  page: page.result.value,
  memory: {
    js_heap_used_mib: mib(heap.usedSize),
    js_heap_total_mib: mib(heap.totalSize),
    embedder_heap_used_mib: mib(heap.embedderHeapUsedSize ?? 0),
    backing_storage_mib: mib(heap.backingStorageSize ?? 0),
    documents: dom.documents,
    nodes: dom.nodes,
    event_listeners: dom.jsEventListeners,
  },
  performance: {
    js_heap_used_mib: mib(metrics.JSHeapUsedSize ?? 0),
    js_heap_total_mib: mib(metrics.JSHeapTotalSize ?? 0),
    dom_nodes: metrics.Nodes ?? 0,
    documents: metrics.Documents ?? 0,
    layout_count: metrics.LayoutCount ?? 0,
    recalculation_count: metrics.RecalcStyleCount ?? 0,
  },
  allocation_sample_seconds: sampleSeconds,
  allocation_sample: allocationSample,
};

socket.close();
console.log(JSON.stringify(output, null, 2));
