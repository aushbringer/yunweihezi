<script setup lang="ts">
import MainView from "./views/MainView.vue";
import DetachedView from "./views/DetachedView.vue";

// Detached terminal windows load the same bundle with ?detach=<sessionId>.
const isDetached = new URLSearchParams(location.search).has("detach");
</script>

<template>
  <DetachedView v-if="isDetached" />
  <MainView v-else />
</template>

<style>
html,
body {
  margin: 0;
  height: 100%;
  /* The window itself is transparent; #app carries the opaque background so
     the UI opacity setting can fade the whole app. */
  background: transparent;
  overflow: hidden;
}
#app {
  margin: 0;
  height: 100%;
  background: #1e1e1e;
  color: #ddd;
  font-family: "Segoe UI", "Microsoft YaHei", sans-serif;
  overflow: hidden;
}
</style>
