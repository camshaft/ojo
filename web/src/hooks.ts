import { useEffect, useState } from "react";
import { Table } from "apache-arrow";
import { dataStore, Query } from "./services/DataStore";

export function useQuery(query: Query): Table | null {
  return useQueryTransform<Table | null>(query, (table: Table | null) => table);
}

export function useQueryTransform<T>(
  query: Query,
  transform: (table: Table | null) => T,
): T {
  const [data, setData] = useState<T>(transform(null));

  useEffect(() => {
    const unsubscribe = dataStore.subscribe(query, (table) => {
      setData(transform ? transform(table) : (table as any));
    });
    return unsubscribe;
  }, [JSON.stringify(query)]);

  return data;
}

export type EventTypesMap = Map<string, EventType>;

export type EventType = {
  id: string;
  name: string;
  description: string;
  module: string;
};

export function useEventTypes(): EventTypesMap {
  return useQueryTransform<EventTypesMap>({ kind: "event_types" }, (table) => {
    const eventTypes = new Map<string, EventType>();
    if (!table) return eventTypes;
    for (const row of table) {
      const obj = row.toJSON();
      eventTypes.set(String(obj.id), {
        id: String(obj.id),
        name: obj.name,
        description: obj.description,
        module: obj.module,
      });
    }
    return eventTypes;
  });
}
