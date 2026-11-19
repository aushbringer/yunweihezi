<script setup lang="ts">
import { computed } from "vue";
import { transfers } from "../../stores/transfers";
import { sftpCancel } from "../../ipc/client";

const items = computed(() => [...transfers.values()].reverse());

function pct(t: { transferred: number; total: number }) {
  if (!t.total) return 0;
  return Math.min(100, Math.round((t.transferred / t.total) * 100));
}

function fmt(n: number) {
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)}K`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)}M`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)}G`;
}

async function cancel(id: string) {
  await sftpCancel(id);
}
</script>

<template>
  <div v-if="items.length" class="queue">
    <div v-for="t in items" :key="t.id" class="qrow">
      <span class="label" :title="t.label">{{ t.label }}</span>
      <div class="bar">
        <div class="fill" :class="t.state" :style="{ width: pct(t) + '%' }" />
      </div>
      <span class="stat">
        <template v-if="t.state === 'running' || t.state === 'queued'">
          {{ fmt(t.transferred) }} / {{ fmt(t.total) }} ({{ pct(t) }}%)
        </template>
        <template v-else-if="t.state === 'done'">完成</template>
        <template v-else-if="t.state === 'cancelled'">已取消</template>
        <template v-else>错误: {{ t.message }}</template>
      </span>
      <a v-if="t.state === 'running' || t.state === 'queued'" class="cancel" @click="cancel(t.id)">✕</a>
    </div>
  </div>
</template>

<style scoped>
.queue {
  border-top: 1px solid #333;
  max-height: 96px;
  overflow-y: auto;
  background: #1e1e1f;
}
.qrow {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 10px;
  font-size: 11.5px;
}
.label {
  flex: 0 0 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #bbb;
}
.bar {
  flex: 1;
  height: 8px;
  background: #333;
  border-radius: 4px;
  overflow: hidden;
}
.fill {
  height: 100%;
  background: #0e639c;
  transition: width 0.2s;
}
.fill.done {
  background: #4ec9b0;
}
.fill.error {
  background: #f14c4c;
}
.fill.cancelled {
  background: #888;
}
.stat {
  flex: 0 0 190px;
  text-align: right;
  color: #999;
}
.cancel {
  color: #999;
  cursor: pointer;
}
.cancel:hover {
  color: #fff;
}
</style>
