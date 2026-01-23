import { Table, tableFromIPC } from "apache-arrow";

export type Query =
  | { kind: "flows"; event_ids?: string[] }
  | { kind: "event_types" }
  | { kind: "stats"; event_ids?: string[] }
  | { kind: "events"; flow_ids?: [string, string][]; event_ids?: string[] };

interface ServerMessage {
  type: "update" | "error";
  id?: number;
  data?: string; // Base64 encoded Arrow IPC
  message?: string;
}

type Listener = (table: Table) => void;

class DataStore {
  private ws: WebSocket | null = null;
  private listeners: Map<number, Set<Listener>> = new Map();
  private cache: Map<number, Table> = new Map();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private unsubscribeTimers: Map<number, ReturnType<typeof setTimeout>> =
    new Map();
  private queryIds: Map<number, Query> = new Map();
  private queryToId: Map<string, number> = new Map();
  private nextQueryId: number = 1;

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
        if (msg.type === "update" && typeof msg.id === "number" && msg.data) {
          // Decode Base64
          const binaryString = atob(msg.data);
          const bytes = new Uint8Array(binaryString.length);
          for (let i = 0; i < binaryString.length; i++) {
            bytes[i] = binaryString.charCodeAt(i);
          }

          const table = tableFromIPC(bytes);
          this.cache.set(msg.id, table);
          this.notify(msg.id, table);
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
    for (const id of this.listeners.keys()) {
      const query = this.queryIds.get(id);
      if (!query) {
        console.error("No query found for id", id);
        continue;
      }
      this.sendSubscribe(id, query);
    }
  }

  private sendSubscribe(id: number, query: Query) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: "subscribe", id, query }));
    }
  }

  private sendUnsubscribe(id: number) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: "unsubscribe", id }));
    }
  }

  subscribe(query: Query, callback: Listener): () => void {
    const key = JSON.stringify(query);
    let id = this.queryToId.get(key);
    if (id === undefined) {
      id = this.nextQueryId++;
      this.queryToId.set(key, id);
      this.queryIds.set(id, query);
    }

    const pending = this.unsubscribeTimers.get(id);
    if (pending) {
      clearTimeout(pending);
      this.unsubscribeTimers.delete(id);
    }
    if (!this.listeners.has(id)) {
      this.listeners.set(id, new Set());
      this.sendSubscribe(id, query);
    }

    const set = this.listeners.get(id)!;
    set.add(callback);

    // If we have cached data, notify immediately
    if (this.cache.has(id)) {
      callback(this.cache.get(id)!);
    }

    return () => {
      set.delete(callback);
      if (set.size === 0) {
        this.listeners.delete(id);
        this.scheduleUnsubscribe(id);
      }
    };
  }

  private scheduleUnsubscribe(id: number) {
    const pending = this.unsubscribeTimers.get(id);
    if (pending) clearTimeout(pending);
    const timer = setTimeout(() => {
      this.sendUnsubscribe(id);
      this.cache.delete(id);
      this.unsubscribeTimers.delete(id);
      this.queryIds.delete(id);
    }, 1000);
    this.unsubscribeTimers.set(id, timer);
  }

  notify(id: number, table: Table) {
    const set = this.listeners.get(id);
    if (set) {
      set.forEach((cb) => cb(table));
    }
  }
}

export const dataStore = new DataStore();
