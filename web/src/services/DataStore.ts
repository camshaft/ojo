import { Table, tableFromIPC } from "apache-arrow";

export type Query =
  | { kind: "flows" }
  | { kind: "event_types" }
  | { kind: "stats" };

interface ServerMessage {
  type: "update" | "error";
  query?: Query;
  data?: string; // Base64 encoded Arrow IPC
  message?: string;
}

type Listener = (table: Table) => void;

class DataStore {
  private ws: WebSocket | null = null;
  private listeners: Map<string, Set<Listener>> = new Map();
  private cache: Map<string, Table> = new Map();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private unsubscribeTimers: Map<string, ReturnType<typeof setTimeout>> =
    new Map();

  constructor() {
    this.connect();
  }

  private connect() {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const host = window.location.host;
    // Connect to /ws relative to current origin (handled by vite proxy or production server)
    this.ws = new WebSocket(`${protocol}//${host}/ws`);

    this.ws.onopen = () => {
      console.log("WS Connection opened");
      this.resubscribe();
    };

    this.ws.onmessage = async (event) => {
      try {
        const msg: ServerMessage = JSON.parse(event.data);
        if (msg.type === "update" && msg.query && msg.data) {
          // Decode Base64
          const binaryString = atob(msg.data);
          const bytes = new Uint8Array(binaryString.length);
          for (let i = 0; i < binaryString.length; i++) {
            bytes[i] = binaryString.charCodeAt(i);
          }

          const key = JSON.stringify(msg.query);
          const table = tableFromIPC(bytes);
          this.cache.set(key, table);
          this.notify(key, table);
        } else if (msg.type === "error") {
          console.error("Server error:", msg.message);
        }
      } catch (e) {
        console.error("Failed to handle message", e);
      }
    };

    this.ws.onclose = () => {
      console.log("Connection closed, retrying...");
      this.ws = null;
      if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
      this.reconnectTimer = setTimeout(() => this.connect(), 2000);
    };
  }

  private resubscribe() {
    for (const type of this.listeners.keys()) {
      this.sendSubscribe(JSON.parse(type));
    }
  }

  private sendSubscribe(query: Query) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: "subscribe", query }));
    }
  }

  private sendUnsubscribe(query: Query) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: "unsubscribe", query }));
    }
  }

  subscribe(query: Query, callback: Listener): () => void {
    const key = JSON.stringify(query);
    const pending = this.unsubscribeTimers.get(key);
    if (pending) {
      clearTimeout(pending);
      this.unsubscribeTimers.delete(key);
    }
    if (!this.listeners.has(key)) {
      this.listeners.set(key, new Set());
      this.sendSubscribe(query);
    }

    const set = this.listeners.get(key)!;
    set.add(callback);

    // If we have cached data, notify immediately
    if (this.cache.has(key)) {
      callback(this.cache.get(key)!);
    }

    return () => {
      set.delete(callback);
      if (set.size === 0) {
        this.listeners.delete(key);
        this.scheduleUnsubscribe(key, query);
      }
    };
  }

  private scheduleUnsubscribe(key: string, query: Query) {
    const pending = this.unsubscribeTimers.get(key);
    if (pending) clearTimeout(pending);
    const timer = setTimeout(() => {
      this.sendUnsubscribe(query);
      this.cache.delete(key);
      this.unsubscribeTimers.delete(key);
    }, 1000);
    this.unsubscribeTimers.set(key, timer);
  }

  notify(key: string, table: Table) {
    const set = this.listeners.get(key);
    if (set) {
      set.forEach((cb) => cb(table));
    }
  }
}

export const dataStore = new DataStore();
