<script setup lang="ts">
import { ref } from "vue";
import { useAppStore } from "../../stores/app";
import { cryptoUnlock } from "../../ipc/client";

/**
 * Full-screen lock screen (会话锁定): opaque overlay that blocks all input
 * while background sessions stay connected. Unlock requires the master
 * password — there is no skip option here by design.
 */
const emit = defineEmits<{ (e: "done"): void }>();
const store = useAppStore();

const password = ref("");
const error = ref("");
const busy = ref(false);

async function submit() {
  error.value = "";
  if (!password.value) return;
  busy.value = true;
  try {
    await cryptoUnlock(password.value);
    emit("done");
  } catch (e) {
    error.value = String(e);
    password.value = "";
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="lock">
    <div class="box">
      <div class="brand">OpenTerm</div>
      <div class="state">已锁定</div>
      <p class="tip">会话在后台保持连接。输入主密码解锁继续操作。</p>
      <input
        v-model="password"
        type="password"
        placeholder="主密码"
        autofocus
        @keyup.enter="submit"
      />
      <div v-if="error" class="err">{{ error }}</div>
      <button class="primary" :disabled="busy || !password" @click="submit">
        {{ busy ? "解锁中…" : "解锁" }}
      </button>
      <div class="tabs-info" v-if="store.tabs.length">
        {{ store.tabs.length }} 个会话保持连接中
      </div>
    </div>
  </div>
</template>

<style scoped>
.lock {
  position: fixed;
  inset: 0;
  background: #1b1b1c;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 300;
}
.box {
  width: 340px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  align-items: stretch;
  text-align: center;
}
.brand {
  font-size: 26px;
  font-weight: 700;
  color: #4fc3f7;
}
.state {
  color: #f7a54f;
  font-size: 14px;
  letter-spacing: 4px;
}
.tip {
  margin: 0;
  color: #888;
  font-size: 12px;
  line-height: 1.6;
}
input {
  background: #252526;
  border: 1px solid #4a4a4a;
  color: #ddd;
  padding: 9px 12px;
  border-radius: 4px;
  font-size: 14px;
  text-align: center;
}
input:focus {
  outline: 1px solid #0e639c;
}
.err {
  color: #f77;
  font-size: 12px;
}
.primary {
  background: #0e639c;
  color: #fff;
  border: none;
  padding: 9px 0;
  border-radius: 4px;
  cursor: pointer;
  font-size: 14px;
}
.primary:disabled {
  opacity: 0.5;
  cursor: default;
}
.tabs-info {
  color: #4ec9b0;
  font-size: 11px;
}
</style>
