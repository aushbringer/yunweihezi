import { reactive } from "vue";
import { sftpWatch } from "../ipc/client";
import { sysNotify } from "../ipc/notify";

export interface TransferItem {
  id: string;
  label: string;
  transferred: number;
  total: number;
  state: string; // queued | running | done | error | cancelled
  message?: string;
}

export const transfers = reactive<Map<string, TransferItem>>(new Map());

let watchBound = false;

/** Bind the single backend transfer event stream (idempotent). */
export function bindTransferWatch() {
  if (watchBound) return;
  watchBound = true;
  void sftpWatch((ev) => {
    const item = transfers.get(ev.id);
    if (!item) return;
    if (ev.kind === "progress") {
      item.transferred = ev.transferred;
      item.total = ev.total;
      item.state = "running";
    } else {
      item.state = ev.state;
      item.message = ev.message;
      if (ev.state === "done") void sysNotify("传输完成", item.label);
      else if (ev.state === "error")
        void sysNotify("传输失败", `${item.label}${ev.message ? `: ${ev.message}` : ""}`);
    }
  });
}

export function trackTransfer(id: string, label: string) {
  transfers.set(id, { id, label, transferred: 0, total: 0, state: "queued" });
}
