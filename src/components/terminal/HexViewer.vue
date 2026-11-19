<script setup lang="ts">
import { nextTick, ref } from "vue";

/**
 * Live hex dump of a session's raw byte stream (debug binary output).
 * Fed via `append(bytes)` from the tab's output handler.
 */

const MAX_LINES = 2000;
const TRIM_TO = 1500;

const lines = ref<string[]>([]);
const scroller = ref<HTMLDivElement | null>(null);

let pending: number[] = [];
let offset = 0;
let scheduled = false;

function flushLine(bytes: number[], off: number, complete: boolean): string {
  const hex = bytes
    .map((b, i) => b.toString(16).padStart(2, "0") + (i === 7 ? "  " : " "))
    .join("")
    .padEnd(16 * 3 + 1, " ");
  const ascii = bytes
    .map((b) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : "."))
    .join("");
  return `${off.toString(16).padStart(8, "0")}  ${hex} |${ascii}${complete ? "" : "…"}|`;
}

function doFlush() {
  scheduled = false;
  const out: string[] = [];
  while (pending.length >= 16) {
    const chunk = pending.splice(0, 16);
    out.push(flushLine(chunk, offset, true));
    offset += 16;
  }
  if (out.length) {
    lines.value.push(...out);
    if (lines.value.length > MAX_LINES) {
      lines.value.splice(0, lines.value.length - TRIM_TO);
    }
    void nextTick(() => {
      const el = scroller.value;
      if (el) el.scrollTop = el.scrollHeight;
    });
  }
}

function append(bytes: Uint8Array) {
  for (let i = 0; i < bytes.length; i++) pending.push(bytes[i]);
  if (!scheduled) {
    scheduled = true;
    // Batch: renders at most once per animation frame.
    requestAnimationFrame(doFlush);
  }
}

/** Flush any partial tail line (e.g. when the view is opened). */
function flushTail() {
  doFlush();
  if (pending.length) {
    lines.value.push(flushLine(pending, offset, false));
    pending = [];
  }
}

function clear() {
  lines.value = [];
  pending = [];
  offset = 0;
}

defineExpose({ append, clear, flushTail });
</script>

<template>
  <div ref="scroller" class="hex">
    <div class="head">偏移量&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;00 01 02 03 04 05 06 07&nbsp;&nbsp;08 09 0A 0B 0C 0D 0E 0F&nbsp;&nbsp;ASCII</div>
    <pre class="body">{{ lines.join("\n") }}</pre>
    <div v-if="!lines.length" class="empty">等待数据…</div>
  </div>
</template>

<style scoped>
.hex {
  position: absolute;
  inset: 0;
  overflow-y: auto;
  background: #141414;
  padding: 6px 10px;
  box-sizing: border-box;
}
.head {
  color: #569cd6;
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  white-space: pre;
  position: sticky;
  top: 0;
  background: #141414;
}
.body {
  color: #b5cea8;
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.4;
  margin: 0;
  white-space: pre;
}
.empty {
  color: #555;
  font-size: 12px;
  margin-top: 8px;
}
</style>
