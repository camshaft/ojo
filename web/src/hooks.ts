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
  value_type: ValueType;
};

export enum ValueType {
  None = 0,
  Identifier = 1,
  Count = 2,
  Bytes = 3,
  Duration = 4,
  RangeCount = 5,
  RangeBytes = 6,
  RangeDuration = 7,
}

export function useEventTypes(): EventTypesMap {
  return useQueryTransform<EventTypesMap>({ kind: "event_types" }, (table) => {
    const eventTypes = new Map<string, EventType>();
    if (!table) return eventTypes;
    for (const row of table) {
      const obj = row.toJSON();

      let valueTypeNum = Number(obj.value_type);
      if (isNaN(valueTypeNum) || valueTypeNum < 0 || valueTypeNum > 7) {
        valueTypeNum = ValueType.None;
      }

      eventTypes.set(String(obj.id), {
        id: String(obj.id),
        name: obj.name,
        description: obj.description,
        module: obj.module,
        value_type: valueTypeNum,
      });
    }
    return eventTypes;
  });
}
