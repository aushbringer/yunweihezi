<script setup lang="ts">
import { computed, ref } from "vue";
import { useAppStore } from "../../stores/app";
import { sessionInputText } from "../../ipc/client";

const store = useAppStore();
const text = ref("");
const clearAfterSend = ref(true);
const error = ref("");

const activeSessionId = computed(
  () => store.tabs.find((t) => t.id === store.activeTabId)?.sessionId ?? ""
);

/** Broadcast mode: compose also honors the broadcast target set. */
const targetCount = computed(() =>
  store.broadcastEnabled ? store.broadcastSessions().length : activeSessionId.value ? 1 : 0
);

async function send() {
  error.value = "";
  if (!text.value || targetCount.value === 0) return;
  const payload = text.value.replace(/\r?\n/g, "\r");
  try {
    const targets = store.broadcastEnabled
      ? store.broadcastSessions()
      : [activeSessionId.value];
    for (const sid of targets) {
      if (sid) await sessionInputText(sid, payload);
    }
    if (clearAfterSend.value) text.value = "";
  } catch (e) {
    error.value = String(e);
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    void send();
  }
}
</script>

<template>
  <div class="compose">
    <textarea
      v-model="text"
      class="box"
      rows="3"
      placeholder="在此撰写命令,可多行;Ctrl+Enter 或点击「发送」发送到当前会话"
      @keydown="onKeydown"
    ></textarea>
    <div class="side">
      <button class="send" :disabled="!text || targetCount === 0" @click="send">
        发送{{ store.broadcastEnabled ? `(${targetCount} 个会话)` : "" }}
      </button>
      <label class="chk">
        <input v-model="clearAfterSend" type="checkbox" />
        发送后清空
      </label>
      <button class="mini" :disabled="!text" @click="text = ''">清空</button>
      <span v-if="error" class="err">{{ error }}</span>
    </div>
  </div>
</template>

<style scoped>
.compose {
  display: flex;
  gap: 8px;
  padding: 6px 8px;
  background: #222223;
  border-top: 1px solid #3c3c3c;
}
.box {
  flex: 1;
  background: #1b1b1c;
  border: 1px solid #3c3c3c;
  border-radius: 4px;
  color: #ddd;
  font-family: Consolas, "Microsoft YaHei", monospace;
  font-size: 12.5px;
  padding: 6px 8px;
  resize: vertical;
  min-height: 44px;
  max-height: 160px;
  outline: none;
}
.side {
  display: flex;
  flex-direction: column;
  gap: 6px;
  align-items: flex-end;
  justify-content: center;
}
.send {
  background: #0e639c;
  border: 1px solid #0e639c;
  color: #fff;
  border-radius: 4px;
  padding: 6px 16px;
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}
.send:disabled {
  opacity: 0.4;
  cursor: default;
}
.chk {
  display: flex;
  align-items: center;
  gap: 4px;
  color: #888;
  font-size: 11px;
  white-space: nowrap;
}
.mini {
  background: #333;
  color: #ccc;
  border: 1px solid #4a4a4a;
  border-radius: 4px;
  font-size: 11px;
  padding: 3px 10px;
  cursor: pointer;
}
.mini:disabled {
  opacity: 0.4;
  cursor: default;
}
.err {
  color: #f14c4c;
  font-size: 11px;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
