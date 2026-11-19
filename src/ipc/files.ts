import { invoke } from "@tauri-apps/api/core";

export const textFileRead = (path: string) => invoke<string>("text_file_read", { path });
export const textFileWrite = (path: string, content: string) =>
  invoke("text_file_write", { path, content });
