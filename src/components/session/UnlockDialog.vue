<script setup lang="ts">
import { ref } from "vue";
import { useAppStore } from "../../stores/app";
import { cryptoSetMaster, cryptoUnlock } from "../../ipc/client";

const emit = defineEmits<{ (e: "done"): void }>();
const store = useAppStore();

const password = ref("");
const confirm = ref("");
const error = ref("");
const busy = ref(false);

async function submit() {
  error.value = "";
  if (!store.masterSet) {
    if (password.value.length < 6) {
      error.value = "主密码至少 6 位";
      return;
    }
    if (password.value !== confirm.value) {
      error.value = "两次输入不一致";
      return;
    }
  }
  busy.value = true;
  try {
    if (store.masterSet) {
      await cryptoUnlock(password.value);
    } else {
      await cryptoSetMaster(password.value);
    }
    emit("done");
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

function skip() {
  // Allow browsing without unlocking; secrets stay inaccessible.
  emit("done");
}
</script>

<template>
  <div class="overlay">
    <div class="dialog">
      <h3>{{ store.masterSet ? "解锁保险库" : "设置主密码" }}</h3>
      <p class="tip">
        {{
          store.masterSet
            ? "输入主密码以解锁会话凭证。"
            : "可选:设置主密码以保护会话凭证(Argon2id + AES-256-GCM)。不设置也可正常使用,凭证将以本机随机密钥加密存储。"
        }}
      </p>
      <input
        v-model="password"
        type="password"
        placeholder="主密码"
        autofocus
        @keyup.enter="submit"
      />
      <input
        v-if="!store.masterSet"
        v-model="confirm"
        type="password"
        placeholder="确认主密码"
        @keyup.enter="submit"
      />
      <div v-if="error" class="err">{{ error }}</div>
      <div class="btns">
        <button class="ghost" @click="skip">暂不解锁</button>
        <button class="primary" :disabled="busy" @click="submit">
          {{ busy ? "处理中…" : store.masterSet ? "解锁" : "设置" }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.65);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 200;
}
.dialog {
  width: 380px;
  background: #2b2b2c;
  border: 1px solid #4a4a4a;
  border-radius: 8px;
  padding: 20px 22px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
.tip {
  margin: 0;
  color: #999;
  font-size: 12px;
  line-height: 1.5;
}
input {
  background: #1e1e1f;
  border: 1px solid #4a4a4a;
  color: #ddd;
  padding: 7px 10px;
  border-radius: 4px;
}
input:focus {
  outline: 1px solid #0e639c;
}
.err {
  color: #f77;
  font-size: 12px;
}
.btns {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.primary {
  background: #0e639c;
  color: #fff;
  border: none;
  padding: 6px 18px;
  border-radius: 4px;
  cursor: pointer;
}
.ghost {
  background: transparent;
  color: #aaa;
  border: 1px solid #555;
  padding: 6px 18px;
  border-radius: 4px;
  cursor: pointer;
}
</style>
